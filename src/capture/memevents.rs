//! Parsing degli eventi *memoria* dal provider kernel `PageFault` (Fase 3).
//!
//! Due segnali ad alto valore, **catturabili su un processo già in esecuzione
//! senza modificarlo** (a differenza del tracing heap a livello `HeapAlloc`, che
//! richiede l'opt-in del target al lancio — vedi docs/07-roadmap.md):
//!
//! - **Hard page fault** (opcode 32, `EVENT_TRACE_FLAG_MEMORY_HARD_FAULTS`): il
//!   page-in da disco, cioè il fault *costoso* che stalla il thread. Porta il TID
//!   e i byte letti → filtrabile sui thread del target.
//! - **VirtualAlloc / VirtualFree** (opcode 98/99, `EVENT_TRACE_FLAG_VIRTUAL_ALLOC`):
//!   le riserve/commit a livello di memoria virtuale (granularità di pagina, non
//!   di `HeapAlloc`). Il payload porta il **PID** → filtrabile sul target. È la
//!   "tracciatura allocazioni" compatibile col vincolo no-injection di Argus.
//!
//! Solo **decode puro** del payload (testabile con buffer sintetici); la sessione
//! che li cattura richiede admin.

use windows::core::GUID;

/// Provider kernel `PageFault` (page fault + VirtualAlloc/Free).
pub const PAGE_FAULT_GUID: GUID = GUID::from_u128(0x3d6fa8d3_fe05_11d0_9dda_00c04fd7ba7c);

/// Opcode dell'evento hard page fault (`PageFault_HardFault`).
pub const OPCODE_HARD_FAULT: u8 = 32;
/// Opcode di VirtualAlloc / VirtualFree (`PageFault_VirtualAlloc`).
pub const OPCODE_VIRTUAL_ALLOC: u8 = 98;
pub const OPCODE_VIRTUAL_FREE: u8 = 99;

/// Un evento di memoria decodificato.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemEvent {
    /// Hard page fault: page-in da disco (costoso). `bytes` letti, `thread_id`.
    HardFault { bytes: u32, thread_id: u32 },
    /// Riserva/commit di memoria virtuale del processo `pid`, `size` byte.
    VirtualAlloc { pid: u32, size: u64 },
    /// Rilascio di memoria virtuale del processo `pid`, `size` byte.
    VirtualFree { pid: u32, size: u64 },
}

/// Decodifica un `PageFault_HardFault`.
///
/// Layout (x64): `InitialTime`(i64)@0, `ReadOffset`(u64)@8, `VirtualAddress`(ptr)@16,
/// `FileObject`(ptr)@16+ptr, `TThreadId`(u32)@16+2·ptr, `ByteCount`(u32)@16+2·ptr+4.
pub fn parse_hard_fault(data: &[u8], pointer_size: usize) -> Option<MemEvent> {
    if !(pointer_size == 4 || pointer_size == 8) {
        return None;
    }
    let tid_off = 16 + 2 * pointer_size;
    let bc_off = tid_off + 4;
    if data.len() < bc_off + 4 {
        return None;
    }
    let thread_id = u32::from_le_bytes(data[tid_off..tid_off + 4].try_into().ok()?);
    let bytes = u32::from_le_bytes(data[bc_off..bc_off + 4].try_into().ok()?);
    Some(MemEvent::HardFault { bytes, thread_id })
}

/// Decodifica un `PageFault_VirtualAlloc` (opcode VirtualAlloc o VirtualFree).
///
/// Layout (x64): `BaseAddress`(ptr)@0, `RegionSize`(ptr)@ptr, `ProcessId`(u32)@2·ptr,
/// `Flags`(u32)@2·ptr+4.
pub fn parse_virtual_alloc(data: &[u8], opcode: u8, pointer_size: usize) -> Option<MemEvent> {
    if !(pointer_size == 4 || pointer_size == 8) {
        return None;
    }
    let size_off = pointer_size;
    let pid_off = 2 * pointer_size;
    if data.len() < pid_off + 4 {
        return None;
    }
    let size = if pointer_size == 8 {
        u64::from_le_bytes(data[size_off..size_off + 8].try_into().ok()?)
    } else {
        u32::from_le_bytes(data[size_off..size_off + 4].try_into().ok()?) as u64
    };
    let pid = u32::from_le_bytes(data[pid_off..pid_off + 4].try_into().ok()?);
    match opcode {
        OPCODE_VIRTUAL_ALLOC => Some(MemEvent::VirtualAlloc { pid, size }),
        OPCODE_VIRTUAL_FREE => Some(MemEvent::VirtualFree { pid, size }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hard_fault_x64() {
        // InitialTime@0, ReadOffset@8, VA@16, FileObject@24, TThreadId@32, ByteCount@36
        let mut v = vec![0u8; 40];
        v[32..36].copy_from_slice(&0x1234u32.to_le_bytes());
        v[36..40].copy_from_slice(&4096u32.to_le_bytes());
        let ev = parse_hard_fault(&v, 8).expect("hard fault valido");
        assert_eq!(
            ev,
            MemEvent::HardFault {
                bytes: 4096,
                thread_id: 0x1234
            }
        );
    }

    #[test]
    fn parses_virtual_alloc_x64() {
        // BaseAddress@0, RegionSize@8, ProcessId@16, Flags@20
        let mut v = vec![0u8; 24];
        v[8..16].copy_from_slice(&(2 * 1024 * 1024u64).to_le_bytes());
        v[16..20].copy_from_slice(&4242u32.to_le_bytes());
        let a = parse_virtual_alloc(&v, OPCODE_VIRTUAL_ALLOC, 8).expect("alloc valida");
        assert_eq!(
            a,
            MemEvent::VirtualAlloc {
                pid: 4242,
                size: 2 * 1024 * 1024
            }
        );
        let f = parse_virtual_alloc(&v, OPCODE_VIRTUAL_FREE, 8).expect("free valida");
        assert_eq!(
            f,
            MemEvent::VirtualFree {
                pid: 4242,
                size: 2 * 1024 * 1024
            }
        );
    }

    #[test]
    fn rejects_short_and_bad_pointer() {
        assert!(parse_hard_fault(&[0u8; 8], 8).is_none());
        assert!(parse_virtual_alloc(&[0u8; 4], OPCODE_VIRTUAL_ALLOC, 8).is_none());
        let v = vec![0u8; 40];
        assert!(parse_hard_fault(&v, 7).is_none());
        assert!(parse_virtual_alloc(&v, 50, 8).is_none()); // opcode non valido
    }
}
