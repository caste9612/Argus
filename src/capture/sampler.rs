//! Il sampler: thread in background che campiona il target a 10 Hz e pubblica
//! `Snapshot` immutabili via `ArcSwap`. Riceve comandi dalla UI via canale.

use crate::aggregation::flame::FlameGraph;
use crate::aggregation::{FlameStatus, ProcessMeta, Snapshot, Status};
use crate::capture::process::{
    image_name, list_processes, open_process, ProcessHandle, ProcessInfo,
};
use crate::capture::profiling::ProfilingSession;
use crate::persist::{self, Capture};
use crate::util::error::ArgusError;
use crate::util::win::{filetime_to_u64, logical_cpu_count};
use arc_swap::ArcSwap;
use crossbeam_channel::{Receiver, RecvTimeoutError};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::mem::size_of;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};
use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows::Win32::System::Threading::{
    GetProcessHandleCount, GetProcessIoCounters, GetProcessTimes, IO_COUNTERS,
};

const TICK: Duration = Duration::from_millis(100);
/// Ogni quanti tick (≈1 s) rinfreschiamo lista processi + conteggio thread.
const REFRESH_EVERY: u64 = 10;

/// Comandi dalla UI verso il sampler.
#[derive(Debug)]
pub enum Command {
    Attach(u32),
    Detach,
    RefreshProcesses,
    /// Salva la sessione corrente su file `.argus` (Fase 4).
    SaveCapture,
    /// Carica una sessione da file `.argus` e la mostra in replay (Fase 4).
    OpenCapture(PathBuf),
    /// Rinfresca l'elenco dei file `.argus` salvati.
    RefreshCaptures,
    Shutdown,
}

/// Stato condiviso pubblicato dal sampler e letto dalla UI.
pub struct Shared {
    pub metrics: ArcSwap<Snapshot>,
    pub processes: ArcSwap<Vec<ProcessInfo>>,
    /// Flame graph degli stack sample ETW (Fase 2). Mutex perché è un albero
    /// mutato di continuo dall'aggregatore e letto dalla UI per il rendering;
    /// le sezioni critiche sono brevissime (add_stack / layout). Vedi D16.
    pub flame: Arc<Mutex<FlameGraph>>,
    /// Elenco dei file `.argus` salvati, dal più recente (Fase 4).
    pub captures: ArcSwap<Vec<PathBuf>>,
}

impl Shared {
    pub fn new() -> Arc<Self> {
        let num_cpus = logical_cpu_count();
        Arc::new(Self {
            metrics: ArcSwap::from_pointee(Snapshot::new(num_cpus, false)),
            processes: ArcSwap::from_pointee(Vec::new()),
            flame: Arc::new(Mutex::new(FlameGraph::new())),
            captures: ArcSwap::from_pointee(Vec::new()),
        })
    }
}

struct Sampler {
    shared: Arc<Shared>,
    snap: Snapshot,
    handle: Option<ProcessHandle>,
    last_list: Arc<Vec<ProcessInfo>>,

    // Baseline per i calcoli a delta.
    last_cpu_100ns: u64,
    last_io_read: u64,
    last_io_write: u64,
    last_sample: Instant,

    // Conteggio thread campionato a 1 Hz, riportato (carry-forward) a 10 Hz.
    thread_cache: u32,

    // Per il CPU% per-processo della lista: tempo CPU cumulativo dell'ultimo
    // refresh, per PID, + istante di quel refresh.
    prev_cpu: HashMap<u32, u64>,
    last_proc_refresh: Instant,

    // Sessione di profiling ETW (flame graph), viva quando si è collegati e la
    // cattura è partita. None se non avviata (es. mancano privilegi admin).
    profiling: Option<ProfilingSession>,
}

impl Sampler {
    fn new(shared: Arc<Shared>) -> Self {
        let num_cpus = logical_cpu_count();
        let has_priv = crate::util::win::enable_debug_privilege();
        if has_priv {
            info!("SeDebugPrivilege attivo");
        }
        let snap = Snapshot::new(num_cpus, has_priv);
        Self {
            shared,
            snap,
            handle: None,
            last_list: Arc::new(Vec::new()),
            last_cpu_100ns: 0,
            last_io_read: 0,
            last_io_write: 0,
            last_sample: Instant::now(),
            thread_cache: 0,
            prev_cpu: HashMap::new(),
            last_proc_refresh: Instant::now(),
            profiling: None,
        }
    }

    fn publish(&self) {
        self.shared.metrics.store(Arc::new(self.snap.clone()));
    }

    fn apply(&mut self, cmd: Command) {
        match cmd {
            Command::Attach(pid) => self.attach(pid),
            Command::Detach => {
                info!(
                    "detach dal PID {:?}",
                    self.snap.attached.as_ref().map(|m| m.pid)
                );
                self.stop_profiling(true);
                self.handle = None;
                self.snap.attached = None;
                self.snap.status = Status::NotAttached;
                self.snap.notice = None;
                self.snap.reset_series();
                self.reset_baselines();
                self.publish();
            }
            Command::RefreshProcesses => {
                self.refresh_processes();
                self.publish();
            }
            Command::SaveCapture => {
                self.save_capture();
                self.publish();
            }
            Command::OpenCapture(path) => {
                self.open_capture(path);
                self.publish();
            }
            Command::RefreshCaptures => self.refresh_captures(),
            Command::Shutdown => {} // gestito nel loop
        }
    }

    fn attach(&mut self, pid: u32) {
        // Stato pulito: ferma un'eventuale sessione precedente e azzera il flame.
        self.stop_profiling(true);
        self.snap.notice = None;
        match open_process(pid) {
            Ok(h) => {
                let name = self
                    .last_list
                    .iter()
                    .find(|p| p.pid == pid)
                    .map(|p| p.name.clone())
                    .or_else(|| image_name(&h))
                    .unwrap_or_else(|| format!("PID {pid}"));
                self.thread_cache = self
                    .last_list
                    .iter()
                    .find(|p| p.pid == pid)
                    .map(|p| p.threads)
                    .unwrap_or(0);

                self.handle = Some(h);
                self.snap.reset_series();
                self.reset_baselines();
                self.snap.attached = Some(ProcessMeta {
                    pid,
                    name: name.clone(),
                });
                self.snap.status = Status::Running;
                info!("collegato a {name} (PID {pid})");
                // Avvia la cattura ETW per il flame graph (richiede admin; se non
                // disponibile, flame_status diventa Unavailable e si prosegue).
                self.start_profiling(pid);
            }
            Err(ArgusError::Permission { hint }) => {
                warn!("attach negato al PID {pid}");
                self.handle = None;
                self.snap.attached = None;
                self.snap.status = Status::Error(hint);
            }
            Err(e) => {
                warn!("attach fallito al PID {pid}: {e}");
                self.handle = None;
                self.snap.attached = None;
                self.snap.status = Status::Error(format!("{e}"));
            }
        }
        self.publish();
    }

    fn reset_baselines(&mut self) {
        self.last_cpu_100ns = 0;
        self.last_io_read = 0;
        self.last_io_write = 0;
        self.last_sample = Instant::now();
        self.thread_cache = 0;
    }

    /// Avvia la cattura ETW del flame graph per `pid`. Presuppone stato pulito
    /// (chiamato da `attach` dopo `stop_profiling`). Senza privilegi admin la
    /// sessione non parte: lo segnaliamo in `flame_status` e si prosegue.
    fn start_profiling(&mut self, pid: u32) {
        match ProfilingSession::start(pid, self.shared.flame.clone()) {
            Ok(sess) => {
                self.profiling = Some(sess);
                self.snap.flame_status = FlameStatus::Active;
                info!("flame graph: cattura ETW avviata per PID {pid}");
            }
            Err(ArgusError::Permission { hint }) => {
                self.snap.flame_status = FlameStatus::Unavailable(hint);
            }
            Err(e) => {
                warn!("flame graph: cattura ETW non avviata: {e}");
                self.snap.flame_status = FlameStatus::Unavailable(format!("{e}"));
            }
        }
    }

    /// Ferma cattura + aggregatore (Drop di `ProfilingSession`). Se `clear_flame`,
    /// azzera anche l'albero; su uscita del target lo si tiene per l'ispezione.
    fn stop_profiling(&mut self, clear_flame: bool) {
        if let Some(sess) = self.profiling.take() {
            drop(sess); // ferma ETW e fa il join dell'aggregatore, in quest'ordine
            info!("flame graph: cattura ETW fermata");
        }
        if clear_flame {
            self.shared.flame.lock().clear();
        }
        self.snap.flame_status = FlameStatus::Off;
    }

    /// Rinfresca l'elenco dei file `.argus` salvati per la UI.
    fn refresh_captures(&self) {
        self.shared
            .captures
            .store(Arc::new(persist::list_captures()));
    }

    /// Salva la sessione corrente (snapshot + flame) su file `.argus`.
    fn save_capture(&mut self) {
        let cap = {
            let flame = self.shared.flame.lock();
            Capture::from_live(&self.snap, &flame, env!("CARGO_PKG_VERSION"))
        };
        match persist::save_capture_file(&cap) {
            Ok(path) => {
                info!("sessione salvata in {path:?}");
                self.snap.notice = Some(format!("Sessione salvata: {}", path.display()));
                self.refresh_captures();
            }
            Err(e) => {
                warn!("salvataggio sessione fallito: {e}");
                self.snap.notice = Some(format!("Salvataggio fallito: {e}"));
            }
        }
    }

    /// Carica una sessione `.argus` e passa al replay statico.
    fn open_capture(&mut self, path: PathBuf) {
        match persist::load_capture_file(&path) {
            Ok(cap) => {
                // Esci dallo stato live: ferma cattura e stacca il target.
                self.stop_profiling(true);
                self.handle = None;
                self.reset_baselines();
                let snap = cap.to_snapshot();
                *self.shared.flame.lock() = cap.flame; // sposta il flame caricato
                self.snap = snap;
                self.snap.notice = Some(format!("Replay: {}", path.display()));
                info!("replay caricato da {path:?}");
            }
            Err(e) => {
                warn!("caricamento sessione fallito: {e}");
                self.snap.notice = Some(format!("Caricamento fallito: {e}"));
            }
        }
    }

    /// Rinfresca la lista processi (1 Hz) e aggiorna il conteggio thread del
    /// target. Un solo snapshot Toolhelp al secondo: overhead trascurabile.
    fn refresh_processes(&mut self) {
        match list_processes() {
            Ok(mut list) => {
                // CPU% per processo = delta del tempo CPU cumulativo dall'ultimo
                // refresh, normalizzato sull'intervallo e sui core logici.
                let now = Instant::now();
                let dt = now
                    .duration_since(self.last_proc_refresh)
                    .as_secs_f32()
                    .max(0.001);
                let ncpu = self.snap.num_cpus as f32;
                for p in &mut list {
                    if let Some(&prev) = self.prev_cpu.get(&p.pid) {
                        let delta = p.cpu_total_100ns.saturating_sub(prev);
                        let pct = ((delta as f32 / 1e7) / dt) * 100.0 / ncpu;
                        p.cpu_percent = pct.clamp(0.0, 100.0);
                    }
                }
                self.prev_cpu = list.iter().map(|p| (p.pid, p.cpu_total_100ns)).collect();
                self.last_proc_refresh = now;

                let arc = Arc::new(list);
                self.shared.processes.store(arc.clone());
                self.last_list = arc;

                // Solo quando siamo live (handle aperto): aggiorna il conteggio
                // thread del target e, se sparito, segnala l'uscita. In replay
                // (handle None) non tocchiamo lo stato.
                if self.handle.is_some() {
                    if let Some(meta) = self.snap.attached.clone() {
                        match self.last_list.iter().find(|p| p.pid == meta.pid) {
                            Some(p) => self.thread_cache = p.threads,
                            None => {
                                info!("il target {} (PID {}) è terminato", meta.name, meta.pid);
                                self.handle = None;
                                self.snap.status = Status::Exited;
                                // Ferma la cattura (target morto) ma tieni il flame
                                // per l'ispezione post-mortem (storie congelate).
                                self.stop_profiling(false);
                            }
                        }
                    }
                }
            }
            Err(e) => debug!("refresh lista processi fallito: {e}"),
        }
    }

    /// Un tick di campionamento: legge le metriche O(1) e aggiorna le storie.
    fn tick(&mut self) {
        if let Some(h) = self.handle.take() {
            match self.sample_once(&h) {
                Ok(()) => {
                    self.handle = Some(h);
                    self.snap.threads = self.thread_cache;
                    self.snap.push_current();
                    self.snap.status = Status::Running;
                }
                Err(e) => {
                    // Trattiamo un errore di campionamento come uscita del target:
                    // congeliamo le storie e segnaliamo lo stato (no panic).
                    debug!("campionamento fallito, assumo target terminato: {e}");
                    self.snap.status = Status::Exited;
                    self.stop_profiling(false);
                }
            }
        }
        self.publish();
    }

    fn sample_once(&mut self, h: &ProcessHandle) -> Result<(), ArgusError> {
        let now = Instant::now();
        let dt = now
            .duration_since(self.last_sample)
            .as_secs_f32()
            .max(0.001);
        let handle = h.raw();

        // SAFETY: handle valido per tutta la durata di queste query read-only.
        unsafe {
            // --- CPU (kernel + user, unità da 100 ns) ---
            let (mut creation, mut exit, mut kernel, mut user) = (
                FILETIME::default(),
                FILETIME::default(),
                FILETIME::default(),
                FILETIME::default(),
            );
            GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user)?;
            let cpu_total = filetime_to_u64(&kernel) + filetime_to_u64(&user);
            if self.last_cpu_100ns > 0 {
                let delta = cpu_total.saturating_sub(self.last_cpu_100ns);
                let cpu_seconds = delta as f32 / 1e7; // 100 ns → s
                let pct = (cpu_seconds / dt) * 100.0 / self.snap.num_cpus as f32;
                self.snap.cpu = pct.clamp(0.0, 100.0);
            }
            self.last_cpu_100ns = cpu_total;

            // --- Memoria ---
            let mut mem = PROCESS_MEMORY_COUNTERS {
                cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
                ..Default::default()
            };
            GetProcessMemoryInfo(handle, &mut mem, mem.cb)?;
            self.snap.working_set_mb = mem.WorkingSetSize as f32 / (1024.0 * 1024.0);
            self.snap.private_mb = mem.PagefileUsage as f32 / (1024.0 * 1024.0);

            // --- I/O ---
            let mut io = IO_COUNTERS::default();
            GetProcessIoCounters(handle, &mut io)?;
            self.snap.total_io_read_mb = io.ReadTransferCount as f32 / (1024.0 * 1024.0);
            self.snap.total_io_write_mb = io.WriteTransferCount as f32 / (1024.0 * 1024.0);
            if self.last_io_read > 0 || self.last_io_write > 0 {
                let r = io.ReadTransferCount.saturating_sub(self.last_io_read);
                let w = io.WriteTransferCount.saturating_sub(self.last_io_write);
                self.snap.io_read_mb_s = (r as f32 / dt) / (1024.0 * 1024.0);
                self.snap.io_write_mb_s = (w as f32 / dt) / (1024.0 * 1024.0);
            }
            self.last_io_read = io.ReadTransferCount;
            self.last_io_write = io.WriteTransferCount;

            // --- Handle ---
            let mut handles: u32 = 0;
            if GetProcessHandleCount(handle, &mut handles).is_ok() {
                self.snap.handles = handles;
            }
        }

        self.last_sample = now;
        Ok(())
    }
}

/// Funzione del thread sampler. Possiede il `Sampler`, cicla finché non riceve
/// `Shutdown` o il canale si chiude.
pub fn run(shared: Arc<Shared>, rx: Receiver<Command>) {
    let mut s = Sampler::new(shared);
    s.refresh_processes();
    s.refresh_captures();
    s.publish();

    let mut last = Instant::now();
    let mut counter: u64 = 0;

    loop {
        match rx.recv_timeout(TICK) {
            Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
            Ok(cmd) => s.apply(cmd),
            Err(RecvTimeoutError::Timeout) => {}
        }

        if last.elapsed() >= TICK {
            last = Instant::now();
            counter = counter.wrapping_add(1);
            if counter.is_multiple_of(REFRESH_EVERY) {
                s.refresh_processes();
            }
            s.tick();
        }
    }

    info!("sampler thread terminato");
}
