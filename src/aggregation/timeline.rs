//! Timeline degli stati dei thread (Fase 3): ricostruisce, dai context switch,
//! i segmenti in cui ciascun thread è stato Running / Ready / Waiting.
//!
//! Per ogni thread si tiene lo stato corrente e da quando; a ogni `CSwitch` il
//! thread uscente (`old_tid`) chiude il suo segmento Running ed entra nello stato
//! `old_state` (in genere Waiting o Ready), mentre l'entrante (`new_tid`) chiude
//! il suo segmento di attesa e inizia a girare. È **pura** (nessuna API Win32):
//! l'alimentazione live arriva dal parser CSwitch via ETW, ma è testabile da sola.

use std::collections::{HashMap, HashSet};

/// Stato di un thread in un intervallo. Mappa i KTHREAD_STATE del kernel sulle
/// categorie utili a leggere il comportamento dello scheduler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadState {
    /// In esecuzione su una CPU.
    Running,
    /// Pronto/in coda ma non in esecuzione (attende una CPU → oversubscription).
    Ready,
    /// In attesa (di I/O, di un lock/evento, …) → possibile contesa.
    Waiting,
    /// Altro (transitorio, terminato, …).
    Other,
}

/// Mappa `OldThreadState` (KTHREAD_STATE) di un CSwitch sullo stato categoria.
pub fn state_from(kthread_state: i8) -> ThreadState {
    match kthread_state {
        1 | 3 | 7 => ThreadState::Ready, // Ready / Standby / DeferredReady
        5 | 6 => ThreadState::Waiting,   // Waiting / Transition
        _ => ThreadState::Other,
    }
}

/// Un segmento temporale di un thread in un certo stato, `[start, end)` (tick QPC).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    pub start: u64,
    pub end: u64,
    pub state: ThreadState,
}

/// Timeline dei segmenti di stato per thread (TID).
#[derive(Default)]
pub struct ThreadTimeline {
    /// Stato corrente per thread: (istante d'inizio, stato).
    cur: HashMap<u32, (u64, ThreadState)>,
    /// Segmenti chiusi per thread.
    segs: HashMap<u32, Vec<Segment>>,
    /// Se presente, si conservano i segmenti solo per questi TID (i thread del
    /// target). Gli altri aggiornano lo stato ma non vengono memorizzati.
    tracked: Option<HashSet<u32>>,
    first: Option<u64>,
    last: u64,
}

impl ThreadTimeline {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.cur.clear();
        self.segs.clear();
        self.tracked = None;
        self.first = None;
        self.last = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.segs.is_empty() && self.cur.is_empty()
    }

    /// Limita i segmenti conservati a questi TID (i thread del target).
    pub fn set_tracked(&mut self, tids: HashSet<u32>) {
        self.tracked = Some(tids);
    }

    fn is_tracked(&self, tid: u32) -> bool {
        self.tracked.as_ref().is_none_or(|s| s.contains(&tid))
    }

    /// Registra un context switch: a `time`, `old_tid` lascia la CPU entrando in
    /// `old_state`, `new_tid` inizia a girare.
    pub fn on_cswitch(&mut self, time: u64, new_tid: u32, old_tid: u32, old_state: i8) {
        // Il thread uscente chiude il suo segmento corrente ed entra in old_state.
        self.transition(old_tid, time, state_from(old_state));
        // Il thread entrante chiude la sua attesa e inizia a girare.
        self.transition(new_tid, time, ThreadState::Running);
        self.first.get_or_insert(time);
        self.last = self.last.max(time);
    }

    /// Imposta lo stato di `tid` a `next` a partire da `time`, chiudendo il
    /// segmento precedente (se sensato e il thread è tracciato).
    fn transition(&mut self, tid: u32, time: u64, next: ThreadState) {
        if tid == 0 {
            return;
        }
        if let Some((start, prev)) = self.cur.insert(tid, (time, next)) {
            if time > start && self.is_tracked(tid) {
                self.segs.entry(tid).or_default().push(Segment {
                    start,
                    end: time,
                    state: prev,
                });
            }
        }
    }

    pub fn span(&self) -> (u64, u64) {
        (self.first.unwrap_or(0), self.last)
    }

    pub fn thread_count(&self) -> usize {
        self.segs.len()
    }

    /// Segmenti di un thread (vuoto se sconosciuto).
    pub fn segments_of(&self, tid: u32) -> &[Segment] {
        self.segs.get(&tid).map_or(&[][..], |v| v.as_slice())
    }

    /// Tempo totale in un dato stato per un thread.
    pub fn time_in(&self, tid: u32, state: ThreadState) -> u64 {
        self.segs.get(&tid).map_or(0, |v| {
            v.iter()
                .filter(|s| s.state == state)
                .map(|s| s.end - s.start)
                .sum()
        })
    }

    /// TID ordinati per tempo Running totale decrescente (i più "caldi" prima).
    pub fn threads_by_busy(&self) -> Vec<(u32, u64)> {
        let mut v: Vec<(u32, u64)> = self
            .segs
            .keys()
            .map(|&tid| (tid, self.time_in(tid, ThreadState::Running)))
            .collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconstructs_running_and_waiting() {
        let mut t = ThreadTimeline::new();
        // T1 gira, poi va in Waiting (old_state=5) mentre T2 parte; poi T1  riparte.
        t.on_cswitch(0, 1, 0, 0); // T1 inizia (old_tid=0 ignorato)
        t.on_cswitch(10, 2, 1, 5); // T1 → Waiting [0,10] running; T2 inizia
        t.on_cswitch(25, 1, 2, 5); // T2 → Waiting [10,25] running; T1 riparte (chiude waiting [10,25])

        // T1: Running [0,10], poi Waiting [10,25].
        assert_eq!(
            t.segments_of(1),
            &[
                Segment {
                    start: 0,
                    end: 10,
                    state: ThreadState::Running
                },
                Segment {
                    start: 10,
                    end: 25,
                    state: ThreadState::Waiting
                },
            ]
        );
        assert_eq!(t.time_in(1, ThreadState::Running), 10);
        assert_eq!(t.time_in(1, ThreadState::Waiting), 15);
        assert_eq!(t.span(), (0, 25));
    }

    #[test]
    fn ready_state_is_distinguished() {
        let mut t = ThreadTimeline::new();
        t.on_cswitch(0, 1, 0, 0);
        t.on_cswitch(5, 2, 1, 1); // T1 esce in Ready (old_state=1)
        assert_eq!(t.segments_of(1)[0].state, ThreadState::Running);
        // T1 è ora Ready; lo si vede quando riparte.
        t.on_cswitch(8, 1, 2, 5);
        assert_eq!(t.time_in(1, ThreadState::Ready), 3); // [5,8] Ready
    }

    #[test]
    fn tracked_filter_and_busy_ranking() {
        let mut t = ThreadTimeline::new();
        t.set_tracked([1, 2].into_iter().collect());
        t.on_cswitch(0, 1, 0, 0);
        t.on_cswitch(100, 999, 1, 5); // T1 running [0,100]; 999 estraneo
        t.on_cswitch(110, 2, 999, 5); // 999 non tracciato → scartato; T2 parte
        t.on_cswitch(130, 1, 2, 5); // T2 running [110,130]
        let busy = t.threads_by_busy();
        assert_eq!(busy[0], (1, 100));
        assert_eq!(busy[1], (2, 20));
        assert!(t.segments_of(999).is_empty());

        t.clear();
        assert!(t.is_empty());
    }
}
