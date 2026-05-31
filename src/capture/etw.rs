//! ETW: cattura degli stack sample CPU dal provider kernel *PerfInfo*.
//!
//! Ogni ~1 ms il kernel campiona il call stack dei thread runnable ed emette un
//! evento `SampleProfile`; con lo stack-walk abilitato, lo stack vero arriva in
//! un evento `StackWalk` collegato. Aggregando questi stack in un flame graph si
//! vede *dove* il processo spende tempo (docs/04-metrics.md).
//!
//! ## Struttura del modulo
//!
//! La sessione ETW kernel-level richiede privilegi di **amministratore** e non è
//! verificabile in test automatici non elevati. Per questo il modulo è diviso:
//!
//! - **Parte pura, testabile**: decodifica del payload binario degli eventi
//!   (`parse_stack_walk`) e i tipi/costanti. È la logica più soggetta a bug
//!   (offset, endianness, dimensione puntatore) ed è coperta da unit test con
//!   buffer sintetici — nessun admin, nessun evento reale.
//! - **Glue della sessione** (`EtwProfiler`): `StartTraceW` + stack-tracing +
//!   `ProcessTrace` su un thread consumer. Codice `unsafe` isolato che degrada
//!   con grazia: se la sessione non parte (manca admin), `start` ritorna `Err`
//!   e Argus prosegue in polling-only con un banner (docs/06-reliability.md).
//!   **Il path di cattura live va verificato manualmente con privilegi elevati**
//!   (vedi `docs/06-reliability.md`, edge case ETW di Fase 2).

use crate::capture::cswitch::{parse_cswitch, OPCODE_CSWITCH, THREAD_GUID};
use crate::util::error::ArgusError;
use crossbeam_channel::Sender;
use std::collections::HashSet;
use std::ffi::c_void;
use std::mem::size_of;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use tracing::{info, warn};
use windows::core::{GUID, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_PRIVILEGE_NOT_HELD, ERROR_SUCCESS,
};
use windows::Win32::System::Diagnostics::Etw::{
    CloseTrace, ControlTraceW, OpenTraceW, ProcessTrace, StartTraceW, TraceSetInformation,
    TraceStackTracingInfo, CLASSIC_EVENT_ID, CONTROLTRACE_HANDLE, EVENT_RECORD,
    EVENT_TRACE_CONTROL_STOP, EVENT_TRACE_FLAG_CSWITCH, EVENT_TRACE_FLAG_PROFILE,
    EVENT_TRACE_LOGFILEW, EVENT_TRACE_LOGFILEW_0, EVENT_TRACE_LOGFILEW_1, EVENT_TRACE_PROPERTIES,
    EVENT_TRACE_REAL_TIME_MODE, PROCESSTRACE_HANDLE, PROCESS_TRACE_MODE_EVENT_RECORD,
    PROCESS_TRACE_MODE_REAL_TIME, WNODE_FLAG_TRACED_GUID,
};

/// GUID di controllo del kernel logger (è il `Wnode.Guid` della sessione di
/// sistema "NT Kernel Logger").
pub const SYSTEM_TRACE_CONTROL_GUID: GUID = GUID::from_u128(0x9e814aad_3204_11d2_9a82_006008a86939);

/// Provider *PerfInfo*: eventi di sample-profile della CPU.
pub const PERFINFO_GUID: GUID = GUID::from_u128(0xce1dbfb4_137e_4da6_87b0_3f59aa102cbc);

/// Evento *StackWalk*: trasporta lo stack associato a un altro evento (qui, al
/// sample-profile della CPU).
pub const STACK_WALK_GUID: GUID = GUID::from_u128(0xdef2fe46_7bd6_4b80_bd94_f57fe20d0ce3);

/// Opcode dell'evento `SampleProfile` del provider PerfInfo (da abilitare per lo
/// stack-walk con `TraceSetInformation(TraceStackTracingInfo, …)`).
pub const OPCODE_SAMPLE_PROFILE: u8 = 46;

/// Nome obbligatorio della sessione kernel classica.
const KERNEL_LOGGER_NAME: &str = "NT Kernel Logger";

/// Uno stack campionato: l'istante (QPC), il processo/thread e gli indirizzi di
/// ritorno. **Ordine leaf-first**, come li fornisce ETW (Stack1 = frame più
/// interno, in esecuzione). Il flame graph li vuole root→leaf: l'aggregatore li
/// inverte (vedi `aggregation::flame`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StackSample {
    pub timestamp: u64,
    pub pid: u32,
    pub tid: u32,
    pub frames: Vec<u64>,
}

/// Un context switch osservato: l'istante, la CPU e i thread coinvolti. Tempo e
/// CPU vengono dall'`EVENT_RECORD` (header + buffer context), non dal payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwitchEvent {
    pub timestamp: u64,
    pub cpu: u16,
    pub new_tid: u32,
    pub old_tid: u32,
    /// Stato in cui passa il thread uscente (KTHREAD_STATE) → Ready/Waiting/…
    pub old_state: i8,
}

/// Evento ETW consegnato all'aggregatore: uno stack sample (flame graph) o un
/// context switch (timeline). Un solo canale preserva l'ordine temporale, che
/// serve alla ricostruzione degli intervalli Running.
#[derive(Clone, Debug)]
pub enum EtwEvent {
    Stack(StackSample),
    Switch(SwitchEvent),
}

/// Dimensione del prefisso fisso del payload StackWalk:
/// `EventTimeStamp`(u64) + `StackProcess`(u32) + `StackThread`(u32).
const STACKWALK_HEADER: usize = 16;

/// Decodifica il payload (`UserData`) di un evento StackWalk in uno
/// `StackSample`.
///
/// `pointer_size` è 8 su un trace a 64 bit, 4 a 32 bit. Ritorna `None` se il
/// buffer è troppo corto o la dimensione puntatore non è valida. Gli indirizzi
/// nulli (padding del kernel quando lo stack reale è più corto del massimo)
/// vengono scartati. Mai panic su input arbitrario (no-panic policy).
pub fn parse_stack_walk(data: &[u8], pointer_size: usize) -> Option<StackSample> {
    if data.len() < STACKWALK_HEADER || !(pointer_size == 4 || pointer_size == 8) {
        return None;
    }
    let timestamp = u64::from_le_bytes(data[0..8].try_into().ok()?);
    let pid = u32::from_le_bytes(data[8..12].try_into().ok()?);
    let tid = u32::from_le_bytes(data[12..16].try_into().ok()?);

    let rest = &data[STACKWALK_HEADER..];
    let n = rest.len() / pointer_size;
    let mut frames = Vec::with_capacity(n);
    for i in 0..n {
        let off = i * pointer_size;
        let addr = if pointer_size == 8 {
            u64::from_le_bytes(rest[off..off + 8].try_into().ok()?)
        } else {
            u32::from_le_bytes(rest[off..off + 4].try_into().ok()?) as u64
        };
        // Indirizzo nullo = padding: scartato (0 non è mai codice valido).
        if addr != 0 {
            frames.push(addr);
        }
    }
    Some(StackSample {
        timestamp,
        pid,
        tid,
        frames,
    })
}

// ============================================================================
// Glue della sessione ETW — richiede admin; il path di cattura live va
// verificato manualmente con privilegi elevati. `unsafe` isolato e commentato.
// ============================================================================

/// `EVENT_TRACE_PROPERTIES` seguito in memoria dal nome del logger, come richiede
/// l'API. La struct è allineata a 8 e la sua dimensione è multipla di 8, quindi
/// `name` cade esattamente a `size_of::<EVENT_TRACE_PROPERTIES>()` (== il valore
/// di `LoggerNameOffset`), senza padding intermedio.
#[repr(C)]
struct KernelTraceProps {
    props: EVENT_TRACE_PROPERTIES,
    name: [u16; 64],
}

impl KernelTraceProps {
    /// Buffer azzerato con i campi comuni impostati (Guid, dimensioni, nome).
    fn new(real_time: bool) -> Self {
        // SAFETY: tutti i campi sono interi/GUID/unioni POD: lo zero è valido.
        let mut k: KernelTraceProps = unsafe { std::mem::zeroed() };
        k.props.Wnode.BufferSize = size_of::<KernelTraceProps>() as u32;
        k.props.Wnode.Guid = SYSTEM_TRACE_CONTROL_GUID;
        k.props.Wnode.Flags = WNODE_FLAG_TRACED_GUID;
        k.props.Wnode.ClientContext = 1; // 1 = clock QPC
        k.props.LoggerNameOffset = size_of::<EVENT_TRACE_PROPERTIES>() as u32;
        if real_time {
            k.props.LogFileMode = EVENT_TRACE_REAL_TIME_MODE;
            // PROFILE = sample-profile CPU (flame); CSWITCH = context switch (timeline).
            k.props.EnableFlags = EVENT_TRACE_FLAG_PROFILE | EVENT_TRACE_FLAG_CSWITCH;
        }
        let wide = KERNEL_LOGGER_NAME.encode_utf16().collect::<Vec<u16>>();
        k.name[..wide.len()].copy_from_slice(&wide);
        k
    }

    #[inline]
    fn as_props_ptr(&mut self) -> *mut EVENT_TRACE_PROPERTIES {
        std::ptr::addr_of_mut!(self.props)
    }
}

/// Contesto passato al callback ETW via `EVENT_TRACE_LOGFILEW.Context`. Vive sul
/// frame del thread consumer per tutta la durata di `ProcessTrace`.
struct ConsumerCtx {
    target_pid: u32,
    /// TID del target, per filtrare i CSwitch (che non portano il PID).
    target_tids: HashSet<u32>,
    tx: Sender<EtwEvent>,
}

/// Profiler ETW: avvia la sessione kernel, abilita lo stack-walk dei sample e
/// consuma gli eventi su un thread dedicato, inviando gli `StackSample` del
/// target via canale. Stop pulito alla Drop.
pub struct EtwProfiler {
    control: CONTROLTRACE_HANDLE,
    /// Handle di consumo (`ProcessTrace`), pubblicato dal thread consumer; serve
    /// a `CloseTrace` per sbloccare `ProcessTrace` allo stop. 0 = non ancora aperto.
    trace: Arc<AtomicU64>,
    join: Option<JoinHandle<()>>,
    stopping: Arc<AtomicBool>,
}

impl EtwProfiler {
    /// Avvia la cattura per `target_pid`. Stack sample (flame) e context switch
    /// (timeline, filtrati su `target_tids`) vengono inviati su `tx` come
    /// `EtwEvent` (canale **bounded**: il callback fa `try_send` e scarta se
    /// pieno, per non stallare il consumer del kernel).
    ///
    /// Ritorna `Err(Permission)` se mancano i privilegi di amministratore — il
    /// chiamante prosegue in polling-only.
    pub fn start(
        target_pid: u32,
        target_tids: HashSet<u32>,
        tx: Sender<EtwEvent>,
    ) -> Result<Self, ArgusError> {
        // La sessione kernel con PROFILE richiede `SeSystemProfilePrivilege`
        // **abilitato** nel token: averlo (da admin) non basta. Senza, StartTrace
        // ritorna 1314 (ERROR_PRIVILEGE_NOT_HELD). Lo attiviamo qui.
        let _ = crate::util::win::enable_privilege("SeSystemProfilePrivilege");

        let mut props = KernelTraceProps::new(true);
        let mut control = CONTROLTRACE_HANDLE::default();

        // SAFETY: `control` e `props` sono validi per la durata della chiamata;
        // `props` ha BufferSize/LoggerNameOffset corretti e il nome in coda.
        let mut status = unsafe {
            StartTraceW(
                &mut control,
                PCWSTR(props.name.as_ptr()),
                props.as_props_ptr(),
            )
        };

        // Sessione kernel **orfana** (es. un Argus precedente chiuso a forza, che
        // non ha eseguito lo stop): fermala per nome e riprova una volta. È un
        // caso reale in sviluppo ETW.
        if status == ERROR_ALREADY_EXISTS {
            let mut stop = KernelTraceProps::new(false);
            // SAFETY: ferma per nome la sessione "NT Kernel Logger" esistente.
            unsafe {
                let _ = ControlTraceW(
                    CONTROLTRACE_HANDLE::default(),
                    PCWSTR(stop.name.as_ptr()),
                    stop.as_props_ptr(),
                    EVENT_TRACE_CONTROL_STOP,
                );
            }
            props = KernelTraceProps::new(true);
            control = CONTROLTRACE_HANDLE::default();
            info!("ETW: trovata sessione kernel orfana, fermata; riprovo StartTrace");
            // SAFETY: come sopra.
            status = unsafe {
                StartTraceW(
                    &mut control,
                    PCWSTR(props.name.as_ptr()),
                    props.as_props_ptr(),
                )
            };
        }

        if status == ERROR_ACCESS_DENIED || status == ERROR_PRIVILEGE_NOT_HELD {
            return Err(ArgusError::Permission {
                hint: "La cattura ETW (flame graph) richiede privilegi di \
                       amministratore. Rilancia Argus come amministratore per \
                       vedere dove il processo spende tempo CPU."
                    .into(),
            });
        } else if status != ERROR_SUCCESS {
            return Err(ArgusError::Internal(format!(
                "StartTrace ha restituito l'errore {} (logger kernel occupato?)",
                status.0
            )));
        }

        // Abilita lo stack-walk per l'evento SampleProfile. Se fallisce, avremmo
        // sample senza stack: degradiamo (log) invece di abortire.
        let mut ev = CLASSIC_EVENT_ID {
            EventGuid: PERFINFO_GUID,
            Type: OPCODE_SAMPLE_PROFILE,
            Reserved: [0; 7],
        };
        // SAFETY: `ev` è una struct locale valida per la durata della chiamata.
        let st = unsafe {
            TraceSetInformation(
                control,
                TraceStackTracingInfo,
                std::ptr::addr_of!(ev) as *const c_void,
                size_of::<CLASSIC_EVENT_ID>() as u32,
            )
        };
        if st != ERROR_SUCCESS {
            warn!(
                "ETW: stack-walk non abilitato (errore {}): flame graph incompleto",
                st.0
            );
        }
        let _ = &mut ev; // mantiene `ev` in vita fino a qui

        // Thread consumer: apre il trace real-time e cicla in ProcessTrace.
        let trace = Arc::new(AtomicU64::new(0));
        let stopping = Arc::new(AtomicBool::new(false));
        let trace_c = trace.clone();
        let join = std::thread::Builder::new()
            .name("argus-etw".into())
            .spawn(move || consume(target_pid, target_tids, tx, trace_c))
            .map_err(|e| ArgusError::Internal(format!("spawn thread ETW fallito: {e}")))?;

        info!("ETW: sessione kernel avviata, cattura stack per PID {target_pid}");
        Ok(Self {
            control,
            trace,
            join: Some(join),
            stopping,
        })
    }

    /// Ferma la sessione e il thread consumer (idempotente).
    pub fn stop(&mut self) {
        if self.stopping.swap(true, Ordering::SeqCst) {
            return;
        }
        // CloseTrace sblocca ProcessTrace nel consumer.
        let h = self.trace.load(Ordering::SeqCst);
        if h != 0 {
            // SAFETY: handle di trace valido pubblicato dal consumer dopo OpenTrace.
            unsafe {
                let _ = CloseTrace(PROCESSTRACE_HANDLE { Value: h });
            }
        }
        // Ferma la sessione kernel.
        let mut props = KernelTraceProps::new(false);
        // SAFETY: control valido; props dimensionato col nome in coda.
        unsafe {
            let _ = ControlTraceW(
                self.control,
                PCWSTR::null(),
                props.as_props_ptr(),
                EVENT_TRACE_CONTROL_STOP,
            );
        }
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
        info!("ETW: sessione kernel fermata");
    }
}

impl Drop for EtwProfiler {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Corpo del thread consumer: apre il trace real-time e gira in `ProcessTrace`
/// (bloccante) finché `CloseTrace` non lo sblocca. `ctx` (e quindi `tx`) vive
/// sul suo stack per tutta la durata della chiamata.
fn consume(
    target_pid: u32,
    target_tids: HashSet<u32>,
    tx: Sender<EtwEvent>,
    trace: Arc<AtomicU64>,
) {
    let ctx = ConsumerCtx {
        target_pid,
        target_tids,
        tx,
    };
    let name: Vec<u16> = KERNEL_LOGGER_NAME
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    // Scrivere i campi (incluse le union) è safe; solo leggerli sarebbe unsafe.
    let mut logfile = EVENT_TRACE_LOGFILEW {
        LoggerName: PWSTR(name.as_ptr() as *mut u16),
        Context: std::ptr::addr_of!(ctx) as *mut c_void,
        Anonymous1: EVENT_TRACE_LOGFILEW_0 {
            ProcessTraceMode: PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD,
        },
        Anonymous2: EVENT_TRACE_LOGFILEW_1 {
            EventRecordCallback: Some(event_callback),
        },
        ..Default::default()
    };

    // SAFETY: logfile valido; LoggerName/Context vivono per tutta la ProcessTrace.
    let handle = unsafe { OpenTraceW(&mut logfile) };
    // Handle invalido = 0xFFFF…: niente da consumare.
    if handle.Value == u64::MAX || handle.Value == 0 {
        warn!("ETW: OpenTrace fallito, nessuno stack verrà raccolto");
        return;
    }
    trace.store(handle.Value, Ordering::SeqCst);

    // SAFETY: handle valido; ProcessTrace blocca finché CloseTrace non lo sblocca.
    let _ = unsafe { ProcessTrace(&[handle], None, None) };
}

/// Callback ETW (ABI di sistema). Instrada gli eventi StackWalk (flame, filtrati
/// per PID) e CSwitch (timeline, filtrati per TID del target), decodificandoli e
/// inviandoli senza bloccare (scarta se il canale è pieno).
unsafe extern "system" fn event_callback(record: *mut EVENT_RECORD) {
    // SAFETY (intera funzione, già contesto `unsafe`): ETW invoca la callback con
    // un `record` valido per la sua durata; `UserData` punta a `UserDataLength`
    // byte e `UserContext` è il puntatore a `ConsumerCtx` messo in
    // `logfile.Context`, vivo per tutta la `ProcessTrace`. Validiamo i null prima
    // di dereferenziare; i campi union (ProcessorIndex) si leggono qui (unsafe).
    if record.is_null() {
        return;
    }
    let r = &*record;
    let ctx = r.UserContext as *const ConsumerCtx;
    if ctx.is_null() || r.UserData.is_null() || r.UserDataLength == 0 {
        return;
    }
    let ctx = &*ctx;
    let data = std::slice::from_raw_parts(r.UserData as *const u8, r.UserDataLength as usize);
    let provider = r.EventHeader.ProviderId;

    if provider == STACK_WALK_GUID {
        if let Some(sample) = parse_stack_walk(data, 8) {
            if sample.pid == ctx.target_pid {
                // try_send: mai bloccare il consumer del kernel; overflow → scarta.
                let _ = ctx.tx.try_send(EtwEvent::Stack(sample));
            }
        }
    } else if provider == THREAD_GUID && r.EventHeader.EventDescriptor.Opcode == OPCODE_CSWITCH {
        if let Some(cs) = parse_cswitch(data) {
            // I CSwitch sono di sistema: teniamo solo quelli dei thread del target.
            if ctx.target_tids.contains(&cs.new_tid) || ctx.target_tids.contains(&cs.old_tid) {
                let ev = SwitchEvent {
                    timestamp: r.EventHeader.TimeStamp as u64,
                    cpu: r.BufferContext.Anonymous.ProcessorIndex,
                    new_tid: cs.new_tid,
                    old_tid: cs.old_tid,
                    old_state: cs.old_state,
                };
                let _ = ctx.tx.try_send(EtwEvent::Switch(ev));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Costruisce un payload StackWalk sintetico con i frame dati.
    fn build(timestamp: u64, pid: u32, tid: u32, frames: &[u64], pointer_size: usize) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&timestamp.to_le_bytes());
        v.extend_from_slice(&pid.to_le_bytes());
        v.extend_from_slice(&tid.to_le_bytes());
        for &f in frames {
            if pointer_size == 8 {
                v.extend_from_slice(&f.to_le_bytes());
            } else {
                v.extend_from_slice(&(f as u32).to_le_bytes());
            }
        }
        v
    }

    #[test]
    fn parses_64bit_stack() {
        let frames = [0xAAAA_u64, 0xBBBB, 0xCCCC];
        let buf = build(0x1122_3344_5566_7788, 0x1234, 0x5678, &frames, 8);
        let s = parse_stack_walk(&buf, 8).expect("payload valido");
        assert_eq!(s.timestamp, 0x1122_3344_5566_7788);
        assert_eq!(s.pid, 0x1234);
        assert_eq!(s.tid, 0x5678);
        assert_eq!(s.frames, frames);
    }

    #[test]
    fn parses_32bit_stack() {
        let frames = [0x0040_1000_u64, 0x0040_2000];
        let buf = build(7, 42, 99, &frames, 4);
        let s = parse_stack_walk(&buf, 4).expect("payload valido");
        assert_eq!(s.pid, 42);
        assert_eq!(s.tid, 99);
        assert_eq!(s.frames, frames);
    }

    #[test]
    fn trailing_null_frames_are_dropped() {
        // Stack reale di 2 frame in un array da 4 (kernel zero-padded).
        let buf = build(1, 1, 1, &[0x1000, 0x2000, 0, 0], 8);
        let s = parse_stack_walk(&buf, 8).unwrap();
        assert_eq!(s.frames, vec![0x1000, 0x2000]);
    }

    #[test]
    fn too_short_buffer_is_none() {
        assert!(parse_stack_walk(&[0u8; 8], 8).is_none());
        assert!(parse_stack_walk(&[], 8).is_none());
    }

    #[test]
    fn invalid_pointer_size_is_none() {
        let buf = build(1, 1, 1, &[0x1000], 8);
        assert!(parse_stack_walk(&buf, 7).is_none());
        assert!(parse_stack_walk(&buf, 0).is_none());
    }

    #[test]
    fn header_only_yields_empty_frames() {
        let buf = build(5, 10, 20, &[], 8);
        let s = parse_stack_walk(&buf, 8).unwrap();
        assert!(s.frames.is_empty());
        assert_eq!(s.pid, 10);
    }

    #[test]
    fn trailing_partial_pointer_is_ignored() {
        // Header + 1 frame da 8 byte + 3 byte spuri: il frame parziale è ignorato.
        let mut buf = build(1, 1, 1, &[0xDEAD_BEEF], 8);
        buf.extend_from_slice(&[1, 2, 3]);
        let s = parse_stack_walk(&buf, 8).unwrap();
        assert_eq!(s.frames, vec![0xDEAD_BEEF]);
    }

    /// La sessione ETW deve comportarsi bene in **entrambi** gli ambienti: se
    /// elevata parte e si ferma pulita (RAII); altrimenti ritorna un errore
    /// controllato. Mai un panic (no-panic policy). Il path di cattura live va
    /// comunque verificato a mano come amministratore.
    #[test]
    fn etw_start_is_graceful_with_or_without_admin() {
        let (tx, _rx) = crossbeam_channel::bounded(64);
        // Elevato: parte e si ferma pulito (RAII). Non elevato: `start` ritorna
        // Err e qui non entriamo. In nessun caso un panic.
        if let Ok(mut p) = EtwProfiler::start(std::process::id(), HashSet::new(), tx) {
            p.stop();
        }
    }
}
