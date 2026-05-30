//! I/O binario little-endian, bounds-checked. Base del formato `.argus`
//! (`persist`) e della serializzazione del flame graph.
//!
//! Scrittura: helper `put_*` che accodano a un `Vec<u8>`.
//! Lettura: `ByteReader`, un cursore che ritorna `Option` (mai panic su input
//! troncato o malevolo — no-panic policy, docs/06-reliability.md).

// --- Scrittura ---

pub fn put_u8(b: &mut Vec<u8>, v: u8) {
    b.push(v);
}

pub fn put_u16(b: &mut Vec<u8>, v: u16) {
    b.extend_from_slice(&v.to_le_bytes());
}

pub fn put_u32(b: &mut Vec<u8>, v: u32) {
    b.extend_from_slice(&v.to_le_bytes());
}

pub fn put_u64(b: &mut Vec<u8>, v: u64) {
    b.extend_from_slice(&v.to_le_bytes());
}

pub fn put_f32(b: &mut Vec<u8>, v: f32) {
    b.extend_from_slice(&v.to_le_bytes());
}

/// Stringa: lunghezza in byte (`u32`) + UTF-8.
pub fn put_str(b: &mut Vec<u8>, s: &str) {
    put_u32(b, s.len() as u32);
    b.extend_from_slice(s.as_bytes());
}

/// Slice di `f32`: conteggio (`u32`) + valori.
pub fn put_f32_slice(b: &mut Vec<u8>, s: &[f32]) {
    put_u32(b, s.len() as u32);
    for &x in s {
        put_f32(b, x);
    }
}

// --- Lettura ---

/// Cursore di lettura bounds-checked. Ogni metodo ritorna `None` se non ci sono
/// abbastanza byte: il chiamante propaga con `?` senza mai panicare.
pub struct ByteReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> ByteReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let slice = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(slice)
    }

    pub fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }

    pub fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }

    pub fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    pub fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }

    pub fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    pub fn string(&mut self) -> Option<String> {
        let n = self.u32()? as usize;
        let bytes = self.take(n)?;
        Some(String::from_utf8_lossy(bytes).into_owned())
    }

    pub fn f32_vec(&mut self) -> Option<Vec<f32>> {
        let n = self.u32()? as usize;
        // Pre-alloc limitato ai byte disponibili: input corrotto non causa OOM.
        let mut v = Vec::with_capacity(n.min(self.remaining() / 4 + 1));
        for _ in 0..n {
            v.push(self.f32()?);
        }
        Some(v)
    }

    /// Verifica che i prossimi byte siano esattamente `expected` (magic header).
    pub fn expect_tag(&mut self, expected: &[u8]) -> bool {
        match self.take(expected.len()) {
            Some(got) => got == expected,
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_primitives() {
        let mut b = Vec::new();
        put_u8(&mut b, 0xAB);
        put_u16(&mut b, 0x1234);
        put_u32(&mut b, 0xDEAD_BEEF);
        put_u64(&mut b, 0x0102_0304_0506_0708);
        put_f32(&mut b, 3.5);
        put_str(&mut b, "ciao αβ");
        put_f32_slice(&mut b, &[1.0, 2.0, -3.0]);

        let mut r = ByteReader::new(&b);
        assert_eq!(r.u8(), Some(0xAB));
        assert_eq!(r.u16(), Some(0x1234));
        assert_eq!(r.u32(), Some(0xDEAD_BEEF));
        assert_eq!(r.u64(), Some(0x0102_0304_0506_0708));
        assert_eq!(r.f32(), Some(3.5));
        assert_eq!(r.string().as_deref(), Some("ciao αβ"));
        assert_eq!(r.f32_vec(), Some(vec![1.0, 2.0, -3.0]));
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn truncated_input_returns_none_not_panic() {
        let b = [1u8, 2, 3]; // troppo corto per un u32
        let mut r = ByteReader::new(&b);
        assert_eq!(r.u32(), None);
        // Una stringa con lunghezza dichiarata enorme non deve panicare né allocare.
        let mut b2 = Vec::new();
        put_u32(&mut b2, 1_000_000);
        b2.extend_from_slice(b"abc");
        let mut r2 = ByteReader::new(&b2);
        assert_eq!(r2.string(), None);
    }

    #[test]
    fn expect_tag_matches_and_rejects() {
        let mut b = Vec::new();
        b.extend_from_slice(b"ARGUSCAP");
        let mut r = ByteReader::new(&b);
        assert!(r.expect_tag(b"ARGUSCAP"));
        let mut r2 = ByteReader::new(&b);
        assert!(!r2.expect_tag(b"OTHER123"));
    }
}
