//! Aggregazione dell'attività disco (Fase 2/3): accumula gli eventi `DiskIo`
//! per direzione (lettura/scrittura) e per disco fisico. È **pura** (nessuna API
//! Win32): l'alimentazione live arriva dal parser DiskIo via ETW, ma è testabile
//! da sola con eventi sintetici.

use crate::capture::diskio::DiskIoEvent;
use std::collections::HashMap;

/// Conteggi cumulativi per una direzione (read o write).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DirStat {
    pub bytes: u64,
    pub ops: u64,
}

impl DirStat {
    fn add(&mut self, bytes: u64) {
        self.bytes += bytes;
        self.ops += 1;
    }

    /// Dimensione media del trasferimento in byte (0 se nessuna operazione).
    pub fn avg_size(&self) -> u64 {
        self.bytes.checked_div(self.ops).unwrap_or(0)
    }
}

/// Statistiche disco aggregate sull'intera cattura. Gli eventi `DiskIo` sono di
/// sistema (non filtrabili per processo): misurano l'attività disco complessiva.
#[derive(Default)]
pub struct DiskStats {
    pub read: DirStat,
    pub write: DirStat,
    /// Per disco fisico: (letture, scritture).
    per_disk: HashMap<u32, (DirStat, DirStat)>,
    /// Somma e massimo dei tempi di risposta grezzi (per una media indicativa).
    resp_sum: u64,
    resp_max: u64,
}

impl DiskStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn is_empty(&self) -> bool {
        self.read.ops == 0 && self.write.ops == 0
    }

    /// Registra un'operazione di disco.
    pub fn on_event(&mut self, ev: &DiskIoEvent) {
        let bytes = ev.bytes as u64;
        let entry = self.per_disk.entry(ev.disk).or_default();
        if ev.is_write {
            self.write.add(bytes);
            entry.1.add(bytes);
        } else {
            self.read.add(bytes);
            entry.0.add(bytes);
        }
        self.resp_sum += ev.response_time;
        self.resp_max = self.resp_max.max(ev.response_time);
    }

    pub fn total_bytes(&self) -> u64 {
        self.read.bytes + self.write.bytes
    }

    pub fn total_ops(&self) -> u64 {
        self.read.ops + self.write.ops
    }

    /// Tempo di risposta medio in unità grezze del kernel (0 se nessuna op).
    pub fn avg_response_raw(&self) -> u64 {
        self.resp_sum.checked_div(self.total_ops()).unwrap_or(0)
    }

    pub fn max_response_raw(&self) -> u64 {
        self.resp_max
    }

    /// Dischi ordinati per byte totali decrescenti: `(disco, letture, scritture)`.
    pub fn disks_by_bytes(&self) -> Vec<(u32, DirStat, DirStat)> {
        let mut v: Vec<(u32, DirStat, DirStat)> = self
            .per_disk
            .iter()
            .map(|(&d, &(r, w))| (d, r, w))
            .collect();
        v.sort_by(|a, b| {
            let ta = a.1.bytes + a.2.bytes;
            let tb = b.1.bytes + b.2.bytes;
            tb.cmp(&ta).then(a.0.cmp(&b.0))
        });
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::diskio::DiskIoEvent;

    fn ev(disk: u32, bytes: u32, is_write: bool, resp: u64) -> DiskIoEvent {
        DiskIoEvent {
            disk,
            bytes,
            offset: 0,
            response_time: resp,
            is_write,
        }
    }

    #[test]
    fn accumulates_by_direction_and_disk() {
        let mut s = DiskStats::new();
        assert!(s.is_empty());
        s.on_event(&ev(0, 4096, false, 10)); // read 4K disco 0
        s.on_event(&ev(0, 8192, false, 30)); // read 8K disco 0
        s.on_event(&ev(1, 16384, true, 20)); // write 16K disco 1

        assert_eq!(s.read.bytes, 12288);
        assert_eq!(s.read.ops, 2);
        assert_eq!(s.read.avg_size(), 6144);
        assert_eq!(s.write.bytes, 16384);
        assert_eq!(s.write.ops, 1);
        assert_eq!(s.total_bytes(), 28672);
        assert_eq!(s.total_ops(), 3);
        assert_eq!(s.avg_response_raw(), 20); // (10+30+20)/3
        assert_eq!(s.max_response_raw(), 30);

        let disks = s.disks_by_bytes();
        // Disco 1 (16K) prima del disco 0 (12K).
        assert_eq!(disks[0].0, 1);
        assert_eq!(disks[0].2.bytes, 16384); // scritture
        assert_eq!(disks[1].0, 0);
        assert_eq!(disks[1].1.bytes, 12288); // letture

        s.clear();
        assert!(s.is_empty());
    }
}
