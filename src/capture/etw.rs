//! Spike ETW (Fase 2) — apertura di una sessione kernel real-time per il
//! profiling a campione (provider `PerfInfo` / `EVENT_TRACE_FLAG_PROFILE`).
//!
//! Scopo dello spike (vedi `docs/07-roadmap.md` Fase 2, `docs/09-decisions.md`
//! D14, `docs/10-etw-spike.md`): validare in `windows-rs` raw — senza nuove
//! dipendenze — che si possa
//!   1. aprire la NT Kernel Logger con il flag PROFILE,
//!   2. impostare l'intervallo di campionamento,
//!   3. (prossimo step) consumare gli eventi di stack walk filtrandoli per PID,
//! il tutto con overhead trascurabile sul target.
//!
//! **Stato di validazione**: il *control path* (start/stop sessione + intervallo
//! di campionamento) è implementato, compila ed è clippy-clean. La cattura vera
//! richiede privilegi di **amministratore** (la NT Kernel Logger è una sessione
//! di sistema unica) e si esegue con `cargo run --features etw --bin etw_spike
//! -- <pid>`. Senza admin `StartTraceW` ritorna `ERROR_ACCESS_DENIED` e lo spike
//! lo riporta con grazia (no panic) — il che valida tutto il path fino alla
//! syscall. Il consumer (`ProcessTrace` + parsing stack) è il prossimo step,
//! documentato in `docs/10-etw-spike.md`.
//!
//! Tutto l'`unsafe` è isolato e commentato `// SAFETY:` come da CLAUDE.md.

use crate::util::error::ArgusError;
use core::ffi::c_void;
use std::mem::size_of;
use tracing::info;
use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Diagnostics::Etw::{
    ControlTraceW, StartTraceW, TraceSetInformation, TraceSampledProfileIntervalInfo,
    CONTROLTRACE_HANDLE, EVENT_TRACE_CONTROL_STOP, EVENT_TRACE_FLAG_PROFILE, EVENT_TRACE_PROPERTIES,
    EVENT_TRACE_REAL_TIME_MODE, KERNEL_LOGGER_NAMEW, TRACE_PROFILE_INTERVAL, WNODE_FLAG_TRACED_GUID,
};
use windows::Win32::System::Diagnostics::Etw::SystemTraceControlGuid;

/// Uno stack sample catturato: PID/TID + indirizzi di ritorno grezzi. La
/// risoluzione in simboli avviene a valle (Fase 2, `capture/symbols.rs`).
#[derive(Clone, Debug)]
#[allow(dead_code)] // popolato dal consumer (prossimo step dello spike, vedi docs/10)
pub struct StackSample {
    pub pid: u32,
    pub tid: u32,
    pub frames: Vec<u64>,
}

/// Periodo di campionamento di default: 10000 × 100 ns = 1 ms (≈ 1 kHz), in
/// linea con la stima di `docs/02-architecture.md`.
const SAMPLE_INTERVAL_100NS: u32 = 10_000;

/// Sessione ETW kernel real-time. Avvia la NT Kernel Logger col provider profile
/// e la **ferma alla Drop** (RAII), come gli altri handle di sistema in `win.rs`.
pub struct KernelTraceSession {
    handle: CONTROLTRACE_HANDLE,
    // Buffer allineato a 8 byte: `EVENT_TRACE_PROPERTIES` seguita dallo spazio
    // per il nome della sessione, che Start/ControlTrace copiano in coda alla
    // struct (a `LoggerNameOffset`). Vec<u64> garantisce l'allineamento richiesto.
    props: Vec<u64>,
}

impl KernelTraceSession {
    /// Avvia la sessione kernel col flag PROFILE. Richiede privilegi di admin.
    pub fn start() -> Result<Self, ArgusError> {
        let prop_size = size_of::<EVENT_TRACE_PROPERTIES>();
        let total_bytes = prop_size + 2 * 1024; // slack ampio per il nome sessione
        let mut props = vec![0u64; total_bytes / 8 + 1];

        // SAFETY: `props` è un Vec<u64> (allineato a 8, come richiede la struct) e
        // più grande di `EVENT_TRACE_PROPERTIES`; scriviamo solo nei suoi limiti.
        // Il puntatore resta valido per tutta la chiamata.
        let handle = unsafe {
            let p = props.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES;
            (*p).Wnode.BufferSize = (props.len() * 8) as u32;
            (*p).Wnode.Flags = WNODE_FLAG_TRACED_GUID;
            (*p).Wnode.ClientContext = 1; // 1 = QPC come sorgente del timestamp
            (*p).Wnode.Guid = SystemTraceControlGuid;
            (*p).EnableFlags = EVENT_TRACE_FLAG_PROFILE;
            (*p).LogFileMode = EVENT_TRACE_REAL_TIME_MODE;
            (*p).LoggerNameOffset = prop_size as u32;

            let mut handle = CONTROLTRACE_HANDLE::default();
            let err = StartTraceW(&mut handle, KERNEL_LOGGER_NAMEW, p);
            if err != ERROR_SUCCESS {
                return Err(map_trace_error(err, "StartTraceW"));
            }
            handle
        };

        info!("sessione ETW kernel avviata");
        let mut session = Self { handle, props };
        session.set_sampling_interval(SAMPLE_INTERVAL_100NS)?;
        Ok(session)
    }

    /// Imposta il periodo di campionamento del profiler (unità da 100 ns).
    fn set_sampling_interval(&mut self, interval_100ns: u32) -> Result<(), ArgusError> {
        let interval = TRACE_PROFILE_INTERVAL {
            Source: 0,
            Interval: interval_100ns,
        };
        // SAFETY: puntatore a una struct locale viva per la durata della chiamata,
        // passata con la sua dimensione. Handle di default (0) = profiler globale.
        let err = unsafe {
            TraceSetInformation(
                CONTROLTRACE_HANDLE::default(),
                TraceSampledProfileIntervalInfo,
                &interval as *const _ as *const c_void,
                size_of::<TRACE_PROFILE_INTERVAL>() as u32,
            )
        };
        if err != ERROR_SUCCESS {
            return Err(map_trace_error(err, "TraceSetInformation(interval)"));
        }
        Ok(())
    }

    /// Ferma la sessione. Chiamata dalla Drop; gli errori sono ignorati (best
    /// effort di teardown: la sessione muore comunque con il processo).
    fn stop(&mut self) {
        // SAFETY: handle valido posseduto in esclusiva da questo wrapper; passiamo
        // lo stesso buffer props (ControlTrace lo riempie con le statistiche).
        unsafe {
            let p = self.props.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES;
            let _ = ControlTraceW(self.handle, KERNEL_LOGGER_NAMEW, p, EVENT_TRACE_CONTROL_STOP);
        }
    }

    // TODO(spike): consumer real-time — prossimo milestone, da validare con admin
    // (dettaglio e razionale in docs/10-etw-spike.md):
    //   1. abilitare lo stack walk sul profile event:
    //      TraceSetInformation(self.handle, TraceStackTracingInfo,
    //          &CLASSIC_EVENT_ID { EventGuid: SampledProfileGuid, Type: 46, .. });
    //   2. EVENT_TRACE_LOGFILEW {
    //          LoggerName: KERNEL_LOGGER_NAMEW (come PWSTR),
    //          Anonymous1.ProcessTraceMode:
    //              PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD,
    //          Anonymous2.EventRecordCallback: Some(on_event) };
    //   3. let h = OpenTraceW(&mut logfile);  (verificare INVALID_PROCESSTRACE_HANDLE)
    //   4. su un thread dedicato: ProcessTrace(&[h], None, None) — blocca finché
    //      la sessione viene fermata (la nostra Drop) → join pulito;
    //   5. extern "system" fn on_event(rec: *mut EVENT_RECORD): filtrare per
    //      (*rec).EventHeader.ProcessId == target e accumulare gli StackWalk in
    //      StackSample. Il callback non può catturare stato → contatori/coda
    //      globali (AtomicU64 + canale), come per il sampler di Fase 1.
}

impl Drop for KernelTraceSession {
    fn drop(&mut self) {
        self.stop();
        info!("sessione ETW kernel fermata");
    }
}

/// Mappa un `WIN32_ERROR` di una API trace in `ArgusError` con contesto utile
/// per l'utente (in particolare il caso "manca admin").
fn map_trace_error(err: WIN32_ERROR, op: &str) -> ArgusError {
    if err == ERROR_ACCESS_DENIED {
        ArgusError::Permission {
            hint: format!(
                "{op}: accesso negato. La sessione ETW kernel richiede privilegi di \
                 amministratore — rilancia come admin."
            ),
        }
    } else {
        ArgusError::Internal(format!("{op} fallita: errore Win32 {}", err.0))
    }
}
