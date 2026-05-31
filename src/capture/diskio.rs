//! Parsing degli eventi *Disk I/O* (provider kernel `DiskIo`, Fase 2/3).
//!
//! Con `EVENT_TRACE_FLAG_DISK_IO` il kernel emette un evento per ogni operazione
//! di lettura/scrittura completata su un disco fisico, con la dimensione del
//! trasferimento, l'offset e il tempo di risposta. Aggregandoli si vede il
//! dettaglio dell'attività disco (throughput, numero di operazioni, dimensione
//! media) durante la cattura — vedi `aggregation::diskstats` (docs/04-metrics.md).
//!
//! Gli eventi `DiskIo` sono **di sistema** (il payload non porta il PID): non
//! sono filtrabili sul target come gli stack. Misurano quindi l'attività disco
//! complessiva mentre si profila — utile per leggere la contesa del disco.
//!
//! Come per gli altri parser ETW, qui vive solo il **decode puro** del payload
//! (testabile con buffer sintetici); la sessione che li cattura richiede admin.

use windows::core::GUID;

/// Provider kernel `DiskIo`.
pub const DISK_IO_GUID: GUID = GUID::from_u128(0x3d6fa8d4_fe05_11d0_9dda_00c04fd7ba7c);

/// Opcode delle operazioni completate (MOF `DiskIo_TypedData`).
pub const OPCODE_DISK_READ: u8 = 10;
pub const OPCODE_DISK_WRITE: u8 = 11;

/// Un'operazione di disco decodificata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiskIoEvent {
    /// Numero del disco fisico.
    pub disk: u32,
    /// Byte trasferiti.
    pub bytes: u32,
    /// Offset in byte sul disco.
    pub offset: u64,
    /// Tempo di risposta dell'operazione (unità grezze del kernel; usato solo in
    /// forma relativa/aggregata, mai presentato come latenza assoluta calibrata).
    pub response_time: u64,
    /// `true` = scrittura, `false` = lettura.
    pub is_write: bool,
}

/// Offset minimo leggibile: fino a `ByteOffset` (@16, 8 byte) → 24 byte.
const DISKIO_MIN: usize = 24;

/// Decodifica il payload di un evento `DiskIo` Read/Write.
///
/// Layout (MOF `DiskIo_TypedData`, x64): `DiskNumber`(u32)@0, `IrpFlags`(u32)@4,
/// `TransferSize`(u32)@8, `Reserved`(u32)@12, `ByteOffset`(i64)@16,
/// `FileObject`(ptr)@24, `Irp`(ptr)@24+ptr, `HighResResponseTime`(u64) subito
/// dopo i due puntatori. Il response time è opzionale (buffer più corti → 0).
/// Ritorna `None` se l'opcode non è Read/Write o il buffer è troppo corto. Mai
/// panic su input arbitrario (no-panic policy).
pub fn parse_disk_io(data: &[u8], opcode: u8, pointer_size: usize) -> Option<DiskIoEvent> {
    let is_write = match opcode {
        OPCODE_DISK_READ => false,
        OPCODE_DISK_WRITE => true,
        _ => return None,
    };
    if data.len() < DISKIO_MIN || !(pointer_size == 4 || pointer_size == 8) {
        return None;
    }
    let disk = u32::from_le_bytes(data[0..4].try_into().ok()?);
    let bytes = u32::from_le_bytes(data[8..12].try_into().ok()?);
    let offset = u64::from_le_bytes(data[16..24].try_into().ok()?);
    // HighResResponseTime sta dopo ByteOffset(8) + FileObject(ptr) + Irp(ptr).
    let rt_off = 24 + 2 * pointer_size;
    let response_time = if data.len() >= rt_off + 8 {
        u64::from_le_bytes(data[rt_off..rt_off + 8].try_into().ok()?)
    } else {
        0
    };
    Some(DiskIoEvent {
        disk,
        bytes,
        offset,
        response_time,
        is_write,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Costruisce un payload DiskIo_TypedData sintetico (x64).
    fn build(disk: u32, bytes: u32, offset: u64, resp: u64) -> Vec<u8> {
        let mut v = vec![0u8; 24 + 2 * 8 + 8]; // fino a HighResResponseTime
        v[0..4].copy_from_slice(&disk.to_le_bytes());
        v[8..12].copy_from_slice(&bytes.to_le_bytes());
        v[16..24].copy_from_slice(&offset.to_le_bytes());
        let rt_off = 24 + 2 * 8;
        v[rt_off..rt_off + 8].copy_from_slice(&resp.to_le_bytes());
        v
    }

    #[test]
    fn parses_read_and_write() {
        let buf = build(2, 65536, 0x1_0000, 4242);
        let r = parse_disk_io(&buf, OPCODE_DISK_READ, 8).expect("read valida");
        assert_eq!(r.disk, 2);
        assert_eq!(r.bytes, 65536);
        assert_eq!(r.offset, 0x1_0000);
        assert_eq!(r.response_time, 4242);
        assert!(!r.is_write);

        let w = parse_disk_io(&buf, OPCODE_DISK_WRITE, 8).expect("write valida");
        assert!(w.is_write);
    }

    #[test]
    fn non_io_opcode_is_none() {
        let buf = build(0, 0, 0, 0);
        assert!(parse_disk_io(&buf, 14, 8).is_none()); // FlushBuffers
        assert!(parse_disk_io(&buf, 0, 8).is_none());
    }

    #[test]
    fn short_buffer_and_bad_pointer_size() {
        assert!(parse_disk_io(&[0u8; 8], OPCODE_DISK_READ, 8).is_none());
        let buf = build(1, 1, 1, 1);
        assert!(parse_disk_io(&buf, OPCODE_DISK_READ, 7).is_none());
    }

    #[test]
    fn missing_response_time_defaults_zero() {
        // Buffer fino a ByteOffset (24 byte): valido, response_time = 0.
        let buf = vec![0u8; 24];
        let r = parse_disk_io(&buf, OPCODE_DISK_READ, 8).expect("valida senza resp time");
        assert_eq!(r.response_time, 0);
    }
}
