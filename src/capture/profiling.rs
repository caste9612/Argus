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

use crate::aggregation::flame::FlameGraph;
use crate::capture::etw::{EtwProfiler, StackSample};
use crate::capture::process::{open_for_symbols, ProcessHandle};
use crate::capture::symbols::SymbolResolver;
use crate::util::error::ArgusError;
use crossbeam_channel::Receiver;
use parking_lot::Mutex;
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
    /// Avvia cattura + aggregazione per `pid`, fondendo gli stack in `flame`.
    pub fn start(pid: u32, flame: Arc<Mutex<FlameGraph>>) -> Result<Self, ArgusError> {
        let (tx, rx) = crossbeam_channel::bounded::<StackSample>(CHANNEL_CAP);
        let etw = EtwProfiler::start(pid, tx)?;

        // Handle per i simboli del target vivo (best-effort): se non si apre, i
        // frame restano indirizzi grezzi (graceful degradation).
        let sym = open_for_symbols(pid).ok();
        let aggregator = std::thread::Builder::new()
            .name("argus-aggregator".into())
            .spawn(move || aggregate(rx, flame, sym))
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
/// l'handle dei simboli, che resta vivo finché il resolver lo usa.
fn aggregate(rx: Receiver<StackSample>, flame: Arc<Mutex<FlameGraph>>, sym: Option<ProcessHandle>) {
    // `resolver` è dichiarato dopo `sym`: alla fine viene droppato per primo
    // (SymCleanup), poi `sym` chiude l'handle — ordine corretto.
    let mut resolver = sym
        .as_ref()
        .and_then(|h| SymbolResolver::for_process(h.raw(), true).ok());

    // Buffer riusato per i nomi risolti del batch (warm path).
    let mut batch: Vec<Vec<Arc<str>>> = Vec::with_capacity(BATCH_MAX);

    while let Ok(first) = rx.recv() {
        batch.clear();
        batch.push(resolve(&mut resolver, &first));
        // Drena ciò che è già pronto per limitare la frequenza di lock.
        while batch.len() < BATCH_MAX {
            match rx.try_recv() {
                Ok(s) => batch.push(resolve(&mut resolver, &s)),
                Err(_) => break,
            }
        }
        // Le risoluzioni costose sono già fatte: il lock copre solo gli add_stack.
        let mut g = flame.lock();
        for names in &batch {
            g.add_stack(names);
        }
    }
    info!("aggregatore flame graph terminato");
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
