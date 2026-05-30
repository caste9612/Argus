//! Timeline degli stati dei thread (Fase 3): ricostruisce gli intervalli in cui
//! ciascun thread è stato *Running* a partire dagli eventi context-switch.
//!
//! Per ogni CPU si tiene il thread attualmente in esecuzione e da quando; a ogni
//! `CSwitch` il thread uscente chiude il suo intervallo e quello entrante ne apre
//! uno nuovo. È **pura** (nessuna API Win32): l'alimentazione live arriva dal
//! parser CSwitch via ETW, ma la logica è testabile in isolamento.

use std::collections::{HashMap, HashSet};

/// Un intervallo di esecuzione `[start, end)` in unità di timestamp ETW (QPC).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Interval {
    pub start: u64,
    pub end: u64,
}

/// Timeline degli intervalli Running per thread (TID).
#[derive(Default)]
pub struct ThreadTimeline {
    /// Per CPU: (thread in esecuzione, istante di inizio).
    running: HashMap<u16, (u32, u64)>,
    /// Per TID: intervalli Running chiusi.
    intervals: HashMap<u32, Vec<Interval>>,
    /// Se presente, si conservano gli intervalli solo per questi TID (i thread
    /// del target). Gli altri servono solo a chiudere correttamente gli intervalli.
    tracked: Option<HashSet<u32>>,
    first: Option<u64>,
    last: u64,
}

impl ThreadTimeline {
    pub fn new() -> Self {
        Self::default()
    }

    /// Azzera la timeline (riuso tra sessioni).
    pub fn clear(&mut self) {
        self.running.clear();
        self.intervals.clear();
        self.tracked = None;
        self.first = None;
        self.last = 0;
    }

    /// Limita gli intervalli conservati a questi TID (i thread del target). I
    /// context switch degli altri thread aggiornano comunque lo stato per CPU,
    /// così gli intervalli dei thread tracciati si chiudono correttamente.
    pub fn set_tracked(&mut self, tids: HashSet<u32>) {
        self.tracked = Some(tids);
    }

    fn is_tracked(&self, tid: u32) -> bool {
        self.tracked.as_ref().is_none_or(|s| s.contains(&tid))
    }

    pub fn is_empty(&self) -> bool {
        self.intervals.is_empty() && self.running.is_empty()
    }

    /// Registra un context switch: su `cpu`, `new_tid` inizia a girare a `time`.
    /// Il thread che girava prima su quella CPU chiude il suo intervallo.
    pub fn on_cswitch(&mut self, time: u64, cpu: u16, new_tid: u32) {
        if let Some((prev_tid, start)) = self.running.insert(cpu, (new_tid, time)) {
            // Chiudi l'intervallo del thread uscente (se sensato e tracciato).
            if prev_tid != 0 && time > start && self.is_tracked(prev_tid) {
                self.intervals
                    .entry(prev_tid)
                    .or_default()
                    .push(Interval { start, end: time });
            }
        }
        self.first.get_or_insert(time);
        self.last = self.last.max(time);
    }

    /// Estremi temporali osservati `(primo, ultimo)`.
    pub fn span(&self) -> (u64, u64) {
        (self.first.unwrap_or(0), self.last)
    }

    /// Numero di thread con almeno un intervallo chiuso.
    pub fn thread_count(&self) -> usize {
        self.intervals.len()
    }

    /// Intervalli Running di un thread (vuoto se sconosciuto).
    pub fn intervals_of(&self, tid: u32) -> &[Interval] {
        self.intervals.get(&tid).map_or(&[][..], |v| v.as_slice())
    }

    /// TID ordinati per tempo Running totale decrescente (i più "caldi" prima).
    pub fn threads_by_busy(&self) -> Vec<(u32, u64)> {
        let mut v: Vec<(u32, u64)> = self
            .intervals
            .iter()
            .map(|(&tid, ints)| (tid, ints.iter().map(|i| i.end - i.start).sum()))
            .collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconstructs_running_intervals_per_cpu() {
        let mut t = ThreadTimeline::new();
        // CPU 0: A da 0, B da 10, A da 25.
        t.on_cswitch(0, 0, 100); // A=100 inizia
        t.on_cswitch(10, 0, 200); // A chiude [0,10], B=200 inizia
        t.on_cswitch(25, 0, 100); // B chiude [10,25], A riparte

        assert_eq!(t.intervals_of(100), &[Interval { start: 0, end: 10 }]);
        assert_eq!(t.intervals_of(200), &[Interval { start: 10, end: 25 }]);
        assert_eq!(t.span(), (0, 25));
        assert_eq!(t.thread_count(), 2);
    }

    #[test]
    fn cpus_are_independent() {
        let mut t = ThreadTimeline::new();
        t.on_cswitch(0, 0, 1); // cpu0: T1
        t.on_cswitch(0, 1, 2); // cpu1: T2 (parallelo)
        t.on_cswitch(50, 0, 9); // cpu0: T1 chiude [0,50]
        t.on_cswitch(80, 1, 9); // cpu1: T2 chiude [0,80]
        assert_eq!(t.intervals_of(1), &[Interval { start: 0, end: 50 }]);
        assert_eq!(t.intervals_of(2), &[Interval { start: 0, end: 80 }]);
    }

    #[test]
    fn tracked_filter_keeps_only_target_threads() {
        let mut t = ThreadTimeline::new();
        t.set_tracked([100, 200].into_iter().collect());
        t.on_cswitch(0, 0, 100); // 100 (target) inizia
        t.on_cswitch(10, 0, 999); // 100 chiude [0,10] (tracked); 999 (estraneo) inizia
        t.on_cswitch(20, 0, 200); // 999 chiude (non tracciato → scartato); 200 inizia
        assert_eq!(t.intervals_of(100), &[Interval { start: 0, end: 10 }]);
        assert!(
            t.intervals_of(999).is_empty(),
            "i thread non-target non vengono conservati"
        );
        assert_eq!(t.thread_count(), 1); // solo 100 ha intervalli chiusi
    }

    #[test]
    fn busy_ranking_and_clear() {
        let mut t = ThreadTimeline::new();
        t.on_cswitch(0, 0, 1);
        t.on_cswitch(100, 0, 2); // T1 busy 100
        t.on_cswitch(110, 0, 1); // T2 busy 10
        let busy = t.threads_by_busy();
        assert_eq!(busy[0], (1, 100));
        assert_eq!(busy[1], (2, 10));

        t.clear();
        assert!(t.is_empty());
        assert_eq!(t.span(), (0, 0));
    }
}
