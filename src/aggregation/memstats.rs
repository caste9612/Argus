//! Aggregazione degli eventi di memoria (Fase 3): hard page fault e
//! VirtualAlloc/Free del target. È **pura** (nessuna API Win32): alimentata dal
//! parser `capture::memevents` via ETW, ma testabile da sola.

use crate::capture::memevents::MemEvent;

/// Statistiche di memoria del target accumulate sulla cattura.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MemStats {
    /// Numero di hard page fault (page-in da disco).
    pub hard_faults: u64,
    /// Byte totali letti dai hard fault.
    pub hard_fault_bytes: u64,
    /// Numero di VirtualAlloc.
    pub valloc_count: u64,
    /// Byte totali riservati/committati da VirtualAlloc.
    pub valloc_bytes: u64,
    /// Numero di VirtualFree.
    pub vfree_count: u64,
    /// Byte totali rilasciati da VirtualFree.
    pub vfree_bytes: u64,
}

impl MemStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn is_empty(&self) -> bool {
        self.hard_faults == 0 && self.valloc_count == 0 && self.vfree_count == 0
    }

    pub fn on_event(&mut self, ev: &MemEvent) {
        match *ev {
            MemEvent::HardFault { bytes, .. } => {
                self.hard_faults += 1;
                self.hard_fault_bytes += bytes as u64;
            }
            MemEvent::VirtualAlloc { size, .. } => {
                self.valloc_count += 1;
                self.valloc_bytes += size;
            }
            MemEvent::VirtualFree { size, .. } => {
                self.vfree_count += 1;
                self.vfree_bytes += size;
            }
        }
    }

    /// Saldo netto (alloc − free) in byte: positivo = crescita della memoria
    /// virtuale riservata durante la cattura (potenziale crescita/leak).
    pub fn net_alloc_bytes(&self) -> i64 {
        self.valloc_bytes as i64 - self.vfree_bytes as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::memevents::MemEvent;

    #[test]
    fn accumulates_faults_and_allocs() {
        let mut s = MemStats::new();
        assert!(s.is_empty());
        s.on_event(&MemEvent::HardFault {
            bytes: 4096,
            thread_id: 1,
        });
        s.on_event(&MemEvent::HardFault {
            bytes: 8192,
            thread_id: 1,
        });
        s.on_event(&MemEvent::VirtualAlloc {
            pid: 1,
            size: 1_048_576,
        });
        s.on_event(&MemEvent::VirtualFree {
            pid: 1,
            size: 262_144,
        });

        assert_eq!(s.hard_faults, 2);
        assert_eq!(s.hard_fault_bytes, 12288);
        assert_eq!(s.valloc_count, 1);
        assert_eq!(s.valloc_bytes, 1_048_576);
        assert_eq!(s.vfree_count, 1);
        assert_eq!(s.net_alloc_bytes(), 1_048_576 - 262_144);
        assert!(!s.is_empty());

        s.clear();
        assert!(s.is_empty());
        assert_eq!(s.net_alloc_bytes(), 0);
    }
}
