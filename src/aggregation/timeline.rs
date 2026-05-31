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

/// Categoria della *causa d'attesa* (KWAIT_REASON) di un segmento Waiting. È la
/// chiave per leggere la **contesa**: tempo in `Lock` alto ⇒ thread che si
/// contendono mutex/eventi; `Io` ⇒ attesa di disco/pagine; `UserIdle` ⇒ attesa
/// volontaria o thread-pool a riposo (non è un problema).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitCategory {
    /// Sincronizzazione: mutex, push lock, eventi, risorse → possibile contesa.
    Lock,
    /// Memoria/paging/I-O: page fault, paging, attesa di trasferimenti.
    Io,
    /// Attesa "sana": richiesta utente, delay, coda thread-pool inattiva.
    UserIdle,
    /// Prelazione dello scheduler / fine quanto (non è vera attesa).
    Preempted,
    /// Non classificato.
    Other,
}

/// Mappa un KWAIT_REASON sulla categoria. Valori dall'enum `KWAIT_REASON` del
/// kernel NT (stabili da anni). Vedi docs/04-metrics.md.
pub fn wait_category(reason: i8) -> WaitCategory {
    match reason {
        // Executive/eventi/lock: dispatcher objects, mutex, push lock, SRW/cond.
        0 | 7 | 14 | 21 | 27 | 28 | 29 | 34 | 35 | 37 => WaitCategory::Lock,
        // Paging / memoria / I-O fisico.
        1 | 2 | 3 | 8 | 9 | 10 | 18 | 19 | 39 | 40 => WaitCategory::Io,
        // Attesa volontaria o coda thread-pool (idle, non contesa).
        4 | 6 | 11 | 13 | 15 => WaitCategory::UserIdle,
        // Prelazione / fine quanto / yield.
        30 | 32 | 33 | 38 => WaitCategory::Preempted,
        _ => WaitCategory::Other,
    }
}

/// Nome leggibile di un KWAIT_REASON (per i tooltip). Sottoinsieme dei valori
/// che capitano di fatto; gli altri ricadono in "(altro)".
pub fn wait_reason_name(reason: i8) -> &'static str {
    match reason {
        0 => "Executive",
        4 => "DelayExecution",
        6 => "UserRequest",
        7 => "WrExecutive",
        13 => "WrUserRequest",
        14 => "WrEventPair",
        15 => "WrQueue",
        16 => "WrLpcReceive",
        17 => "WrLpcReply",
        21 => "WrKeyedEvent",
        27 => "WrResource",
        28 => "WrPushLock",
        29 => "WrMutex",
        30 => "WrQuantumEnd",
        32 => "WrPreempted",
        33 => "WrYieldExecution",
        34 => "WrFastMutex",
        35 => "WrGuardedMutex",
        37 => "WrAlertByThreadId",
        _ => "(altro)",
    }
}

/// Tempo totale (tick QPC) speso in attesa, suddiviso per categoria di causa.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WaitBreakdown {
    pub lock: u64,
    pub io: u64,
    pub user_idle: u64,
    pub preempted: u64,
    pub other: u64,
}

impl WaitBreakdown {
    pub fn total(&self) -> u64 {
        self.lock + self.io + self.user_idle + self.preempted + self.other
    }

    fn add(&mut self, cat: WaitCategory, dt: u64) {
        match cat {
            WaitCategory::Lock => self.lock += dt,
            WaitCategory::Io => self.io += dt,
            WaitCategory::UserIdle => self.user_idle += dt,
            WaitCategory::Preempted => self.preempted += dt,
            WaitCategory::Other => self.other += dt,
        }
    }
}

/// Un segmento temporale di un thread in un certo stato, `[start, end)` (tick QPC).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    pub start: u64,
    pub end: u64,
    pub state: ThreadState,
    /// KWAIT_REASON del segmento, significativo solo se `state == Waiting`.
    pub wait_reason: i8,
}

/// Timeline dei segmenti di stato per thread (TID).
#[derive(Default)]
pub struct ThreadTimeline {
    /// Stato corrente per thread: (istante d'inizio, stato, causa-attesa).
    cur: HashMap<u32, (u64, ThreadState, i8)>,
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
    /// `old_state` per il motivo `old_wait_reason`, `new_tid` inizia a girare.
    pub fn on_cswitch(
        &mut self,
        time: u64,
        new_tid: u32,
        old_tid: u32,
        old_state: i8,
        old_wait_reason: i8,
    ) {
        // Il thread uscente chiude il suo segmento corrente ed entra in old_state;
        // la causa-attesa si attribuisce al nuovo segmento (significativa se Waiting).
        self.transition(old_tid, time, state_from(old_state), old_wait_reason);
        // Il thread entrante chiude la sua attesa e inizia a girare.
        self.transition(new_tid, time, ThreadState::Running, 0);
        self.first.get_or_insert(time);
        self.last = self.last.max(time);
    }

    /// Imposta lo stato di `tid` a `next` (con causa `reason`) a partire da
    /// `time`, chiudendo il segmento precedente (se sensato e il thread è tracciato).
    fn transition(&mut self, tid: u32, time: u64, next: ThreadState, reason: i8) {
        if tid == 0 {
            return;
        }
        if let Some((start, prev, prev_reason)) = self.cur.insert(tid, (time, next, reason)) {
            if time > start && self.is_tracked(tid) {
                self.segs.entry(tid).or_default().push(Segment {
                    start,
                    end: time,
                    state: prev,
                    wait_reason: prev_reason,
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

    /// Suddivisione del tempo di **attesa** per categoria di causa, su tutti i
    /// thread tracciati. Sintetizza la contesa: `lock` alto = thread bloccati su
    /// sincronizzazione (vedi `05-ui-design`, pannello attese).
    pub fn wait_breakdown(&self) -> WaitBreakdown {
        let mut b = WaitBreakdown::default();
        for segs in self.segs.values() {
            for s in segs {
                if s.state == ThreadState::Waiting {
                    b.add(wait_category(s.wait_reason), s.end - s.start);
                }
            }
        }
        b
    }

    /// Suddivisione delle attese per un singolo thread.
    pub fn wait_breakdown_of(&self, tid: u32) -> WaitBreakdown {
        let mut b = WaitBreakdown::default();
        if let Some(segs) = self.segs.get(&tid) {
            for s in segs {
                if s.state == ThreadState::Waiting {
                    b.add(wait_category(s.wait_reason), s.end - s.start);
                }
            }
        }
        b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconstructs_running_and_waiting() {
        let mut t = ThreadTimeline::new();
        // T1 gira, poi va in Waiting (old_state=5) mentre T2 parte; poi T1  riparte.
        t.on_cswitch(0, 1, 0, 0, 0); // T1 inizia (old_tid=0 ignorato)
        t.on_cswitch(10, 2, 1, 5, 29); // T1 → Waiting [0,10] running; T2 inizia (causa WrMutex)
        t.on_cswitch(25, 1, 2, 5, 0); // T2 → Waiting [10,25] running; T1 riparte (chiude waiting [10,25])

        // T1: Running [0,10], poi Waiting [10,25] (causa WrMutex=29).
        assert_eq!(
            t.segments_of(1),
            &[
                Segment {
                    start: 0,
                    end: 10,
                    state: ThreadState::Running,
                    wait_reason: 0,
                },
                Segment {
                    start: 10,
                    end: 25,
                    state: ThreadState::Waiting,
                    wait_reason: 29,
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
        t.on_cswitch(0, 1, 0, 0, 0);
        t.on_cswitch(5, 2, 1, 1, 0); // T1 esce in Ready (old_state=1)
        assert_eq!(t.segments_of(1)[0].state, ThreadState::Running);
        // T1 è ora Ready; lo si vede quando riparte.
        t.on_cswitch(8, 1, 2, 5, 0);
        assert_eq!(t.time_in(1, ThreadState::Ready), 3); // [5,8] Ready
    }

    #[test]
    fn tracked_filter_and_busy_ranking() {
        let mut t = ThreadTimeline::new();
        t.set_tracked([1, 2].into_iter().collect());
        t.on_cswitch(0, 1, 0, 0, 0);
        t.on_cswitch(100, 999, 1, 5, 0); // T1 running [0,100]; 999 estraneo
        t.on_cswitch(110, 2, 999, 5, 0); // 999 non tracciato → scartato; T2 parte
        t.on_cswitch(130, 1, 2, 5, 0); // T2 running [110,130]
        let busy = t.threads_by_busy();
        assert_eq!(busy[0], (1, 100));
        assert_eq!(busy[1], (2, 20));
        assert!(t.segments_of(999).is_empty());

        t.clear();
        assert!(t.is_empty());
    }

    #[test]
    fn wait_breakdown_classifies_by_reason() {
        let mut t = ThreadTimeline::new();
        t.set_tracked([1].into_iter().collect()); // isola T1 (T2 è solo "l'altro")
                                                  // T1 gira [0,10], poi attende su mutex (WrMutex=29) [10,30] → Lock.
        t.on_cswitch(0, 1, 0, 0, 0);
        t.on_cswitch(10, 2, 1, 5, 29);
        t.on_cswitch(30, 1, 2, 5, 0); // T1 riparte: chiude Waiting[10,30] causa 29
                                      // T1 gira [30,40], poi attende su I/O paging (PageIn=2) [40,55] → Io.
        t.on_cswitch(40, 2, 1, 5, 2);
        t.on_cswitch(55, 1, 2, 5, 0);
        let b = t.wait_breakdown();
        assert_eq!(b.lock, 20); // [10,30]
        assert_eq!(b.io, 15); // [40,55]
        assert_eq!(b.user_idle, 0);
        assert_eq!(b.total(), 35);
        assert_eq!(t.wait_breakdown_of(1).lock, 20);
    }

    #[test]
    fn wait_category_mapping() {
        assert_eq!(wait_category(29), WaitCategory::Lock); // WrMutex
        assert_eq!(wait_category(34), WaitCategory::Lock); // WrFastMutex
        assert_eq!(wait_category(2), WaitCategory::Io); // PageIn
        assert_eq!(wait_category(6), WaitCategory::UserIdle); // UserRequest
        assert_eq!(wait_category(32), WaitCategory::Preempted); // WrPreempted
    }
}
