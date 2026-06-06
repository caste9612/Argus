//! Parsing degli eventi *context switch* (provider kernel `Thread`, Fase 3).
//!
//! Ogni cambio di thread schedulato sulla CPU genera un evento `CSwitch`.
//! Aggregandoli si ricostruisce la timeline degli stati dei thread
//! (Running/Ready/Waiting) — vedi `aggregation::timeline` (docs/04-metrics.md).
//!
//! Come per `etw::parse_stack_walk`, qui vive solo il **decode puro** del payload
//! (testabile con buffer sintetici); la sessione che li cattura richiede admin.

use windows::core::GUID;

/// Provider kernel `Thread` (eventi CSwitch, ThreadStart, …).
pub const THREAD_GUID: GUID = GUID::from_u128(0x3d6fa8d1_fe05_11d0_9dda_00c04fd7ba7c);

/// Opcode dell'evento `CSwitch` del provider Thread.
pub const OPCODE_CSWITCH: u8 = 36;

/// Stato del thread che lascia la CPU (campo `OldThreadState`). Valori del
/// kernel (KTHREAD_STATE); ci interessano soprattutto Waiting/Ready/Terminated.
pub const STATE_WAITING: i8 = 5;
pub const STATE_TERMINATED: i8 = 4;

/// Un evento context-switch decodificato. Il **tempo** e la **CPU** non sono nel
/// payload: arrivano dall'`EVENT_RECORD` (header + `BufferContext`), e vengono
/// passati a parte all'aggregatore.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CSwitch {
    /// Thread che inizia a girare sulla CPU.
    pub new_tid: u32,
    /// Thread che lascia la CPU.
    pub old_tid: u32,
    /// Stato in cui passa il thread uscente (vedi `STATE_*`).
    pub old_state: i8,
    /// Motivo dell'attesa del thread uscente (KWAIT_REASON).
    pub old_wait_reason: i8,
}

/// Dimensione dei campi fissi del payload CSwitch che leggiamo (fino a
/// `OldThreadState` @14): NewTid(4)+OldTid(4)+priorità/stati(≤8).
const CSWITCH_MIN: usize = 16;

/// Decodifica il payload (`UserData`) di un evento CSwitch.
///
/// Layout (MOF `CSwitch`): NewThreadId(u32) @0, OldThreadId(u32) @4,
/// NewThreadPriority(i8) @8, OldThreadPriority(i8) @9, PreviousCState(u8) @10,
/// SpareByte @11, OldThreadWaitReason(i8) @12, OldThreadWaitMode @13,
/// OldThreadState(i8) @14, … Ritorna `None` se il buffer è troppo corto (mai
/// panic).
pub fn parse_cswitch(data: &[u8]) -> Option<CSwitch> {
    if data.len() < CSWITCH_MIN {
        return None;
    }
    let new_tid = u32::from_le_bytes(data[0..4].try_into().ok()?);
    let old_tid = u32::from_le_bytes(data[4..8].try_into().ok()?);
    let old_wait_reason = data[12] as i8;
    let old_state = data[14] as i8;
    Some(CSwitch {
        new_tid,
        old_tid,
        old_state,
        old_wait_reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(new_tid: u32, old_tid: u32, wait_reason: i8, state: i8) -> Vec<u8> {
        let mut v = vec![0u8; 24];
        v[0..4].copy_from_slice(&new_tid.to_le_bytes());
        v[4..8].copy_from_slice(&old_tid.to_le_bytes());
        v[12] = wait_reason as u8;
        v[14] = state as u8;
        v
    }

    #[test]
    fn parses_cswitch_fields() {
        let buf = build(0x1111, 0x2222, 6, STATE_WAITING);
        let c = parse_cswitch(&buf).expect("payload valido");
        assert_eq!(c.new_tid, 0x1111);
        assert_eq!(c.old_tid, 0x2222);
        assert_eq!(c.old_wait_reason, 6);
        assert_eq!(c.old_state, STATE_WAITING);
    }

    #[test]
    fn too_short_is_none() {
        assert!(parse_cswitch(&[0u8; 8]).is_none());
        assert!(parse_cswitch(&[]).is_none());
        // Esattamente al minimo: ok.
        assert!(parse_cswitch(&[0u8; 16]).is_some());
    }
}
