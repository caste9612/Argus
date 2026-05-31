//! Sessione di profiling del flame graph: lega insieme cattura ETW, risoluzione
//! simboli e aggregazione (Fase 2).
//!
//! `ProfilingSession` possiede la sessione ETW e un **thread aggregatore**
//! dedicato (D3): il thread drena gli `StackSample` dal canale ETW, risolve gli
//! indirizzi in nomi (cache LRU, fuori dal lock) e li fonde nel `FlameGraph`
//! condiviso con la UI. La risoluzione costosa avviene fuori dal lock; il lock
//! sul flame è preso solo per i rapidi `add_stack` in batch.
//!
//! La cattura ETW richiede privilegi di amministratore: `start` propaga
//! `Err(Permission)` se mancano, e il chiamante mostra un banner (polling-only).

use crate::aggregation::diskstats::DiskStats;
use crate::aggregation::flame::FlameGraph;
use crate::aggregation::timeline::ThreadTimeline;
use crate::capture::diskio::DiskIoEvent;
use crate::capture::etw::{EtwEvent, EtwProfiler, StackSample, SwitchEvent};
use crate::capture::process::{open_for_symbols, thread_ids, ProcessHandle};
use crate::capture::symbols::SymbolResolver;
use crate::util::error::ArgusError;
use crossbeam_channel::Receiver;
use parking_lot::Mutex;
use std::collections::HashSet;
use std::sync::Arc;
use std::thread::JoinHandle;
use tracing::info;

/// Capacità del canale ETW→aggregatore. Bounded: in overflow il callback ETW
/// scarta (mai bloccare il consumer del kernel).
const CHANNEL_CAP: usize = 8192;

/// Quanti stack al massimo si aggregano sotto un singolo lock del flame.
const BATCH_MAX: usize = 256;

/// Sessione di profiling attiva. Alla Drop ferma ETW e l'aggregatore, in
/// quest'ordine, così il thread aggregatore esce pulito (canale disconnesso).
pub struct ProfilingSession {
    etw: EtwProfiler,
    aggregator: Option<JoinHandle<()>>,
}

impl ProfilingSession {
    /// Avvia cattura + aggregazione per `pid`: gli stack vanno in `flame`, i
    /// context-switch (filtrati sui thread del target) nella `timeline`.
    pub fn start(
        pid: u32,
        flame: Arc<Mutex<FlameGraph>>,
        timeline: Arc<Mutex<ThreadTimeline>>,
        disk: Arc<Mutex<DiskStats>>,
    ) -> Result<Self, ArgusError> {
        let (tx, rx) = crossbeam_channel::bounded::<EtwEvent>(CHANNEL_CAP);
        // TID del target ora (snapshot): i CSwitch ETW non portano il PID.
        let tids: HashSet<u32> = thread_ids(pid).into_iter().collect();
        // La timeline conserva gli intervalli solo per i thread del target.
        timeline.lock().set_tracked(tids.clone());
        let etw = EtwProfiler::start(pid, tids, tx)?;

        // Handle per i simboli del target vivo (best-effort): se non si apre, i
        // frame restano indirizzi grezzi (graceful degradation).
        let sym = open_for_symbols(pid).ok();
        let aggregator = std::thread::Builder::new()
            .name("argus-aggregator".into())
            .spawn(move || aggregate(rx, flame, timeline, disk, sym))
            .map_err(|e| ArgusError::Internal(format!("spawn aggregatore fallito: {e}")))?;

        Ok(Self {
            etw,
            aggregator: Some(aggregator),
        })
    }
}

impl Drop for ProfilingSession {
    fn drop(&mut self) {
        // 1) Ferma ETW: il thread consumer termina e rilascia il Sender, così il
        //    canale si disconnette e l'aggregatore esce dal `recv`.
        self.etw.stop();
        // 2) Attendi l'aggregatore.
        if let Some(j) = self.aggregator.take() {
            let _ = j.join();
        }
    }
}

/// Corpo del thread aggregatore. Possiede il resolver (DbgHelp è per-thread) e
/// l'handle dei simboli, che resta vivo finché il resolver lo usa. Instrada gli
/// stack nel flame e i context-switch nella timeline, prendendo i lock solo per
/// i rapidi inserimenti in batch (la risoluzione simboli è fuori dal lock).
fn aggregate(
    rx: Receiver<EtwEvent>,
    flame: Arc<Mutex<FlameGraph>>,
    timeline: Arc<Mutex<ThreadTimeline>>,
    disk: Arc<Mutex<DiskStats>>,
    sym: Option<ProcessHandle>,
) {
    // `resolver` è dichiarato dopo `sym`: alla fine viene droppato per primo
    // (SymCleanup), poi `sym` chiude l'handle — ordine corretto.
    let mut resolver = sym
        .as_ref()
        .and_then(|h| SymbolResolver::for_process(h.raw(), true).ok());

    // Buffer riusati per il batch (warm path), preservando l'ordine.
    let mut stacks: Vec<Vec<Arc<str>>> = Vec::with_capacity(BATCH_MAX);
    let mut switches: Vec<SwitchEvent> = Vec::with_capacity(BATCH_MAX);
    let mut disks: Vec<DiskIoEvent> = Vec::with_capacity(BATCH_MAX);

    // Esce quando il canale si chiude (ETW fermato).
    while let Ok(first) = rx.recv() {
        stacks.clear();
        switches.clear();
        disks.clear();
        classify(&mut resolver, first, &mut stacks, &mut switches, &mut disks);
        // Drena ciò che è già pronto per limitare la frequenza di lock.
        for _ in 1..BATCH_MAX {
            match rx.try_recv() {
                Ok(e) => classify(&mut resolver, e, &mut stacks, &mut switches, &mut disks),
                Err(_) => break,
            }
        }
        if !stacks.is_empty() {
            let mut g = flame.lock();
            for names in &stacks {
                g.add_stack(names);
            }
        }
        if !switches.is_empty() {
            let mut t = timeline.lock();
            for sw in &switches {
                t.on_cswitch(
                    sw.timestamp,
                    sw.new_tid,
                    sw.old_tid,
                    sw.old_state,
                    sw.old_wait_reason,
                );
            }
        }
        if !disks.is_empty() {
            let mut d = disk.lock();
            for ev in &disks {
                d.on_event(ev);
            }
        }
    }
    info!("aggregatore ETW terminato");
}

/// Smista un evento: stack risolto (→ flame), context-switch (→ timeline) o
/// operazione di disco (→ diskstats).
fn classify(
    resolver: &mut Option<SymbolResolver>,
    event: EtwEvent,
    stacks: &mut Vec<Vec<Arc<str>>>,
    switches: &mut Vec<SwitchEvent>,
    disks: &mut Vec<DiskIoEvent>,
) {
    match event {
        EtwEvent::Stack(s) => stacks.push(resolve(resolver, &s)),
        EtwEvent::Switch(sw) => switches.push(sw),
        EtwEvent::Disk(ev) => disks.push(ev),
    }
}

/// Risolve uno stack in nomi, invertendo l'ordine ETW (leaf-first) in root→leaf
/// come vuole il flame graph. Senza resolver, usa l'indirizzo grezzo.
fn resolve(resolver: &mut Option<SymbolResolver>, sample: &StackSample) -> Vec<Arc<str>> {
    sample
        .frames
        .iter()
        .rev()
        .map(|&addr| match resolver.as_mut() {
            Some(r) => r.resolve(addr),
            None => Arc::from(format!("0x{addr:016x}")),
        })
        .collect()
}
