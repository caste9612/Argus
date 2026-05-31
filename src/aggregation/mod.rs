//! Layer 3 — Aggregation.
//!
//! In Fase 1 il polling produce già valori scalari, quindi qui vive lo stato
//! time-series (`Snapshot`) e gli helper sulle storie. In Fase 2 si aggiunge il
//! flame graph (`flame`), che aggrega gli stack sample ETW in un albero pesato.
//! L'aggregatore come thread separato arriverà con gli eventi ad alta frequenza.

pub mod diskstats;
pub mod flame;
pub mod timeline;

use std::collections::VecDeque;

/// Campioni di storia mantenuti: 600 @ 10 Hz = 60 secondi visibili.
pub const HISTORY_LEN: usize = 600;
/// Frequenza di campionamento base.
pub const SAMPLE_HZ: f32 = 10.0;

/// Metadati del processo collegato.
#[derive(Clone, Default)]
pub struct ProcessMeta {
    pub pid: u32,
    pub name: String,
}

/// Stato corrente della sessione di profiling.
#[derive(Clone, PartialEq)]
pub enum Status {
    /// Nessun processo collegato.
    NotAttached,
    /// Collegato e in campionamento.
    Running,
    /// Il processo target è terminato durante la sessione.
    Exited,
    /// Sessione caricata da file `.argus` (replay statico); etichetta per la UI.
    Replay(String),
    /// Un'operazione è fallita; messaggio pronto per l'utente.
    Error(String),
}

/// Stato della cattura ETW per il flame graph (Fase 2).
#[derive(Clone, PartialEq)]
pub enum FlameStatus {
    /// Profiling non attivo (non collegati, o sessione ETW non avviata).
    Off,
    /// ETW non disponibile: motivo pronto per l'utente (es. mancano privilegi).
    Unavailable(String),
    /// Cattura in corso.
    Active,
}

/// Vista immutabile pubblicata dal sampler verso la UI a ~10 Hz.
///
/// Viene clonata ad ogni tick e scambiata atomicamente via `ArcSwap`: la UI
/// legge sempre uno stato consistente senza mai bloccarsi. Il costo del clone
/// (~7 × 600 `f32` ≈ 17 KB) è trascurabile a 10 Hz.
#[derive(Clone)]
pub struct Snapshot {
    pub num_cpus: u32,
    pub has_debug_privilege: bool,
    pub attached: Option<ProcessMeta>,
    pub status: Status,
    /// Stato della cattura ETW per il flame graph.
    pub flame_status: FlameStatus,
    /// Messaggio transitorio per la UI (es. "Salvato in …" o errore di caricamento).
    pub notice: Option<String>,

    // Valori correnti (per le KPI card).
    pub cpu: f32,
    pub working_set_mb: f32,
    pub private_mb: f32,
    pub threads: u32,
    pub handles: u32,
    pub io_read_mb_s: f32,
    pub io_write_mb_s: f32,
    pub total_io_read_mb: f32,
    pub total_io_write_mb: f32,

    // Storie (più vecchio davanti, più recente in fondo).
    pub cpu_hist: VecDeque<f32>,
    pub ws_hist: VecDeque<f32>,
    pub priv_hist: VecDeque<f32>,
    pub io_r_hist: VecDeque<f32>,
    pub io_w_hist: VecDeque<f32>,
    pub thread_hist: VecDeque<f32>,
    pub handle_hist: VecDeque<f32>,
}

impl Snapshot {
    pub fn new(num_cpus: u32, has_debug_privilege: bool) -> Self {
        Self {
            num_cpus,
            has_debug_privilege,
            attached: None,
            status: Status::NotAttached,
            flame_status: FlameStatus::Off,
            notice: None,
            cpu: 0.0,
            working_set_mb: 0.0,
            private_mb: 0.0,
            threads: 0,
            handles: 0,
            io_read_mb_s: 0.0,
            io_write_mb_s: 0.0,
            total_io_read_mb: 0.0,
            total_io_write_mb: 0.0,
            cpu_hist: VecDeque::with_capacity(HISTORY_LEN),
            ws_hist: VecDeque::with_capacity(HISTORY_LEN),
            priv_hist: VecDeque::with_capacity(HISTORY_LEN),
            io_r_hist: VecDeque::with_capacity(HISTORY_LEN),
            io_w_hist: VecDeque::with_capacity(HISTORY_LEN),
            thread_hist: VecDeque::with_capacity(HISTORY_LEN),
            handle_hist: VecDeque::with_capacity(HISTORY_LEN),
        }
    }

    /// Azzera storie e valori correnti (nuovo attach), preservando i metadati di
    /// sistema (num_cpus, privilegi).
    pub fn reset_series(&mut self) {
        self.cpu = 0.0;
        self.working_set_mb = 0.0;
        self.private_mb = 0.0;
        self.threads = 0;
        self.handles = 0;
        self.io_read_mb_s = 0.0;
        self.io_write_mb_s = 0.0;
        self.total_io_read_mb = 0.0;
        self.total_io_write_mb = 0.0;
        self.cpu_hist.clear();
        self.ws_hist.clear();
        self.priv_hist.clear();
        self.io_r_hist.clear();
        self.io_w_hist.clear();
        self.thread_hist.clear();
        self.handle_hist.clear();
    }

    /// Spinge i valori correnti nelle storie, mantenendo la lunghezza massima.
    pub fn push_current(&mut self) {
        push_capped(&mut self.cpu_hist, self.cpu);
        push_capped(&mut self.ws_hist, self.working_set_mb);
        push_capped(&mut self.priv_hist, self.private_mb);
        push_capped(&mut self.io_r_hist, self.io_read_mb_s);
        push_capped(&mut self.io_w_hist, self.io_write_mb_s);
        push_capped(&mut self.thread_hist, self.threads as f32);
        push_capped(&mut self.handle_hist, self.handles as f32);
    }
}

/// Inserisce in coda mantenendo al massimo `HISTORY_LEN` elementi.
fn push_capped(v: &mut VecDeque<f32>, x: f32) {
    if v.len() >= HISTORY_LEN {
        v.pop_front();
    }
    v.push_back(x);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_is_capped() {
        let mut s = Snapshot::new(8, false);
        for i in 0..(HISTORY_LEN + 50) {
            s.cpu = i as f32;
            s.push_current();
        }
        assert_eq!(s.cpu_hist.len(), HISTORY_LEN);
        // Il più recente è in fondo.
        assert_eq!(*s.cpu_hist.back().unwrap(), (HISTORY_LEN + 49) as f32);
    }

    #[test]
    fn reset_clears_everything() {
        let mut s = Snapshot::new(8, false);
        s.cpu = 50.0;
        s.push_current();
        s.reset_series();
        assert!(s.cpu_hist.is_empty());
        assert_eq!(s.cpu, 0.0);
    }
}
