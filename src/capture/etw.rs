//! ETW: cattura degli stack sample CPU dal provider kernel *PerfInfo*.
//!
//! Ogni ~1 ms il kernel campiona il call stack dei thread runnable ed emette un
//! evento `SampleProfile`; con lo stack-walk abilitato, lo stack vero arriva in
//! un evento `StackWalk` collegato. Aggregando questi stack in un flame graph si
//! vede *dove* il processo spende tempo (docs/04-metrics.md).
//!
//! ## Struttura del modulo
//!
//! La sessione ETW kernel-level richiede privilegi di **amministratore** e non è
//! verificabile in test automatici non elevati. Per questo il modulo è diviso:
//!
//! - **Parte pura, testabile** (questo file, sotto): decodifica del payload
//!   binario degli eventi (`parse_stack_walk`) e i tipi/costanti. È la logica
//!   più soggetta a bug (offset, endianness, dimensione puntatore) ed è coperta
//!   da unit test con buffer sintetici — nessun admin, nessun evento reale.
//! - **Glue della sessione** (in arrivo): `StartTraceW` + stack-tracing +
//!   `ProcessTrace`. Codice `unsafe` isolato, che degrada con grazia se la
//!   sessione non parte (manca admin) → Argus continua in polling-only con un
//!   banner (docs/06-reliability.md).

use windows::core::GUID;

/// GUID di controllo del kernel logger (è il `Wnode.Guid` della sessione di
/// sistema "NT Kernel Logger").
pub const SYSTEM_TRACE_CONTROL_GUID: GUID = GUID::from_u128(0x9e814aad_3204_11d2_9a82_006008a86939);

/// Provider *PerfInfo*: eventi di sample-profile della CPU.
pub const PERFINFO_GUID: GUID = GUID::from_u128(0xce1dbfb4_137e_4da6_87b0_3f59aa102cbc);

/// Evento *StackWalk*: trasporta lo stack associato a un altro evento (qui, al
/// sample-profile della CPU).
pub const STACK_WALK_GUID: GUID = GUID::from_u128(0xdef2fe46_7bd6_4b80_bd94_f57fe20d0ce3);

/// Opcode dell'evento `SampleProfile` del provider PerfInfo (da abilitare per lo
/// stack-walk con `TraceSetInformation(TraceStackTracingInfo, …)`).
pub const OPCODE_SAMPLE_PROFILE: u8 = 46;

/// Uno stack campionato: l'istante (QPC), il processo/thread e gli indirizzi di
/// ritorno. **Ordine leaf-first**, come li fornisce ETW (Stack1 = frame più
/// interno, in esecuzione). Il flame graph li vuole root→leaf: l'aggregatore li
/// inverte (vedi `aggregation::flame`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StackSample {
    pub timestamp: u64,
    pub pid: u32,
    pub tid: u32,
    pub frames: Vec<u64>,
}

/// Dimensione del prefisso fisso del payload StackWalk:
/// `EventTimeStamp`(u64) + `StackProcess`(u32) + `StackThread`(u32).
const STACKWALK_HEADER: usize = 16;

/// Decodifica il payload (`UserData`) di un evento StackWalk in uno
/// `StackSample`.
///
/// `pointer_size` è 8 su un trace a 64 bit, 4 a 32 bit. Ritorna `None` se il
/// buffer è troppo corto o la dimensione puntatore non è valida. Gli indirizzi
/// nulli (padding del kernel quando lo stack reale è più corto del massimo)
/// vengono scartati. Mai panic su input arbitrario (no-panic policy).
pub fn parse_stack_walk(data: &[u8], pointer_size: usize) -> Option<StackSample> {
    if data.len() < STACKWALK_HEADER || !(pointer_size == 4 || pointer_size == 8) {
        return None;
    }
    let timestamp = u64::from_le_bytes(data[0..8].try_into().ok()?);
    let pid = u32::from_le_bytes(data[8..12].try_into().ok()?);
    let tid = u32::from_le_bytes(data[12..16].try_into().ok()?);

    let rest = &data[STACKWALK_HEADER..];
    let n = rest.len() / pointer_size;
    let mut frames = Vec::with_capacity(n);
    for i in 0..n {
        let off = i * pointer_size;
        let addr = if pointer_size == 8 {
            u64::from_le_bytes(rest[off..off + 8].try_into().ok()?)
        } else {
            u32::from_le_bytes(rest[off..off + 4].try_into().ok()?) as u64
        };
        // Indirizzo nullo = padding: scartato (0 non è mai codice valido).
        if addr != 0 {
            frames.push(addr);
        }
    }
    Some(StackSample {
        timestamp,
        pid,
        tid,
        frames,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Costruisce un payload StackWalk sintetico con i frame dati.
    fn build(timestamp: u64, pid: u32, tid: u32, frames: &[u64], pointer_size: usize) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&timestamp.to_le_bytes());
        v.extend_from_slice(&pid.to_le_bytes());
        v.extend_from_slice(&tid.to_le_bytes());
        for &f in frames {
            if pointer_size == 8 {
                v.extend_from_slice(&f.to_le_bytes());
            } else {
                v.extend_from_slice(&(f as u32).to_le_bytes());
            }
        }
        v
    }

    #[test]
    fn parses_64bit_stack() {
        let frames = [0xAAAA_u64, 0xBBBB, 0xCCCC];
        let buf = build(0x1122_3344_5566_7788, 0x1234, 0x5678, &frames, 8);
        let s = parse_stack_walk(&buf, 8).expect("payload valido");
        assert_eq!(s.timestamp, 0x1122_3344_5566_7788);
        assert_eq!(s.pid, 0x1234);
        assert_eq!(s.tid, 0x5678);
        assert_eq!(s.frames, frames);
    }

    #[test]
    fn parses_32bit_stack() {
        let frames = [0x0040_1000_u64, 0x0040_2000];
        let buf = build(7, 42, 99, &frames, 4);
        let s = parse_stack_walk(&buf, 4).expect("payload valido");
        assert_eq!(s.pid, 42);
        assert_eq!(s.tid, 99);
        assert_eq!(s.frames, frames);
    }

    #[test]
    fn trailing_null_frames_are_dropped() {
        // Stack reale di 2 frame in un array da 4 (kernel zero-padded).
        let buf = build(1, 1, 1, &[0x1000, 0x2000, 0, 0], 8);
        let s = parse_stack_walk(&buf, 8).unwrap();
        assert_eq!(s.frames, vec![0x1000, 0x2000]);
    }

    #[test]
    fn too_short_buffer_is_none() {
        assert!(parse_stack_walk(&[0u8; 8], 8).is_none());
        assert!(parse_stack_walk(&[], 8).is_none());
    }

    #[test]
    fn invalid_pointer_size_is_none() {
        let buf = build(1, 1, 1, &[0x1000], 8);
        assert!(parse_stack_walk(&buf, 7).is_none());
        assert!(parse_stack_walk(&buf, 0).is_none());
    }

    #[test]
    fn header_only_yields_empty_frames() {
        let buf = build(5, 10, 20, &[], 8);
        let s = parse_stack_walk(&buf, 8).unwrap();
        assert!(s.frames.is_empty());
        assert_eq!(s.pid, 10);
    }

    #[test]
    fn trailing_partial_pointer_is_ignored() {
        // Header + 1 frame da 8 byte + 3 byte spuri: il frame parziale è ignorato.
        let mut buf = build(1, 1, 1, &[0xDEAD_BEEF], 8);
        buf.extend_from_slice(&[1, 2, 3]);
        let s = parse_stack_walk(&buf, 8).unwrap();
        assert_eq!(s.frames, vec![0xDEAD_BEEF]);
    }
}
