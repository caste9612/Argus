//! Formato `.argus`: persistenza di una sessione di profiling (Fase 4).
//!
//! Binario, little-endian, **versionato** con magic header. Serializzazione
//! **manuale** (niente serde): i dati sono semplici, il round-trip è interamente
//! testabile e non aggiunge dipendenze di serializzazione (coerente con la
//! disciplina sulle dipendenze del progetto). La compressione (zstd) è rinviata
//! a una versione successiva del formato — vedi D18 in docs/09-decisions.md: il
//! byte `compression` nell'header lascia spazio a introdurla senza rotture.

use crate::aggregation::flame::{FlameGraph, NodeId};
use crate::util::bytes::{
    put_f32, put_f32_slice, put_str, put_u16, put_u32, put_u64, put_u8, ByteReader,
};
use crate::util::error::ArgusError;
use std::collections::HashMap;

const MAGIC: &[u8; 8] = b"ARGUSCAP";
const FORMAT_VERSION: u16 = 1;
const COMPRESSION_NONE: u8 = 0;

/// Una sessione catturata, sufficiente a riprodurla (replay statico): metadati,
/// storie time-series e flame graph. Le storie seguono la convenzione del
/// `Snapshot` (più vecchio davanti, più recente in fondo).
pub struct Capture {
    pub argus_version: String,
    pub process_name: String,
    pub pid: u32,
    pub num_cpus: u32,
    pub cpu_hist: Vec<f32>,
    pub ws_hist: Vec<f32>,
    pub priv_hist: Vec<f32>,
    pub io_r_hist: Vec<f32>,
    pub io_w_hist: Vec<f32>,
    pub thread_hist: Vec<f32>,
    pub handle_hist: Vec<f32>,
    pub total_io_read_mb: f32,
    pub total_io_write_mb: f32,
    pub flame: FlameGraph,
}

/// Serializza una `Capture` nel formato `.argus` (v1, non compresso).
pub fn write_capture(c: &Capture) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend_from_slice(MAGIC);
    put_u16(&mut b, FORMAT_VERSION);
    put_u8(&mut b, COMPRESSION_NONE);

    put_str(&mut b, &c.argus_version);
    put_str(&mut b, &c.process_name);
    put_u32(&mut b, c.pid);
    put_u32(&mut b, c.num_cpus);

    put_f32_slice(&mut b, &c.cpu_hist);
    put_f32_slice(&mut b, &c.ws_hist);
    put_f32_slice(&mut b, &c.priv_hist);
    put_f32_slice(&mut b, &c.io_r_hist);
    put_f32_slice(&mut b, &c.io_w_hist);
    put_f32_slice(&mut b, &c.thread_hist);
    put_f32_slice(&mut b, &c.handle_hist);
    put_f32(&mut b, c.total_io_read_mb);
    put_f32(&mut b, c.total_io_write_mb);

    write_flame(&mut b, &c.flame);
    b
}

/// Deserializza una `Capture`. Errore (mai panic) su magic errato, versione non
/// supportata, o file troncato/corrotto.
pub fn read_capture(data: &[u8]) -> Result<Capture, ArgusError> {
    let mut r = ByteReader::new(data);
    if !r.expect_tag(MAGIC) {
        return Err(ArgusError::Internal(
            "Non sembra un file .argus (intestazione errata).".into(),
        ));
    }
    let version = r.u16().ok_or_else(corrupt)?;
    if version != FORMAT_VERSION {
        return Err(ArgusError::Internal(format!(
            "Versione del formato .argus non supportata: {version} (attesa {FORMAT_VERSION})."
        )));
    }
    let _compression = r.u8().ok_or_else(corrupt)?; // 0 = nessuna (per ora)

    let argus_version = r.string().ok_or_else(corrupt)?;
    let process_name = r.string().ok_or_else(corrupt)?;
    let pid = r.u32().ok_or_else(corrupt)?;
    let num_cpus = r.u32().ok_or_else(corrupt)?;

    let cpu_hist = r.f32_vec().ok_or_else(corrupt)?;
    let ws_hist = r.f32_vec().ok_or_else(corrupt)?;
    let priv_hist = r.f32_vec().ok_or_else(corrupt)?;
    let io_r_hist = r.f32_vec().ok_or_else(corrupt)?;
    let io_w_hist = r.f32_vec().ok_or_else(corrupt)?;
    let thread_hist = r.f32_vec().ok_or_else(corrupt)?;
    let handle_hist = r.f32_vec().ok_or_else(corrupt)?;
    let total_io_read_mb = r.f32().ok_or_else(corrupt)?;
    let total_io_write_mb = r.f32().ok_or_else(corrupt)?;

    let flame = read_flame(&mut r).ok_or_else(corrupt)?;

    Ok(Capture {
        argus_version,
        process_name,
        pid,
        num_cpus,
        cpu_hist,
        ws_hist,
        priv_hist,
        io_r_hist,
        io_w_hist,
        thread_hist,
        handle_hist,
        total_io_read_mb,
        total_io_write_mb,
        flame,
    })
}

fn corrupt() -> ArgusError {
    ArgusError::Internal("File .argus troncato o danneggiato.".into())
}

/// Scrive il flame graph: tabella nomi deduplicata + nodi (name_id, genitore,
/// profondità, total, own) + total_samples.
fn write_flame(b: &mut Vec<u8>, g: &FlameGraph) {
    let n = g.node_count();
    let mut names: Vec<&str> = Vec::new();
    let mut index: HashMap<&str, u32> = HashMap::new();
    let mut node_name_id: Vec<u32> = Vec::with_capacity(n);
    for i in 0..n as NodeId {
        let nm = g.name_of(i);
        let id = *index.entry(nm).or_insert_with(|| {
            let id = names.len() as u32;
            names.push(nm);
            id
        });
        node_name_id.push(id);
    }

    put_u32(b, names.len() as u32);
    for nm in &names {
        put_str(b, nm);
    }
    put_u32(b, n as u32);
    for (i, &nid) in node_name_id.iter().enumerate() {
        let id = i as NodeId;
        put_u32(b, nid);
        put_u32(b, g.parent_of(id));
        put_u16(b, g.depth_of(id));
        put_u64(b, g.total_of(id));
        put_u64(b, g.own_of(id));
    }
    put_u64(b, g.total_samples());
}

fn read_flame(r: &mut ByteReader) -> Option<FlameGraph> {
    let name_count = r.u32()? as usize;
    let mut names = Vec::with_capacity(name_count.min(r.remaining() / 2 + 1));
    for _ in 0..name_count {
        names.push(r.string()?);
    }
    let node_count = r.u32()? as usize;
    let mut nodes: Vec<(String, NodeId, u16, u64, u64)> =
        Vec::with_capacity(node_count.min(r.remaining() / 8 + 1));
    for _ in 0..node_count {
        let name_id = r.u32()? as usize;
        let parent = r.u32()?;
        let depth = r.u16()?;
        let total = r.u64()?;
        let own = r.u64()?;
        let name = names.get(name_id).cloned().unwrap_or_default();
        nodes.push((name, parent, depth, total, own));
    }
    let total_samples = r.u64()?;
    Some(FlameGraph::from_nodes(&nodes, total_samples))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_capture() -> Capture {
        let mut flame = FlameGraph::new();
        flame.add_stack(&["main", "compute", "inner"]);
        flame.add_stack(&["main", "compute", "inner"]);
        flame.add_stack(&["main", "compute"]);
        flame.add_stack(&["main", "io_wait"]);
        Capture {
            argus_version: "0.1.0".into(),
            process_name: "fixture.exe".into(),
            pid: 1234,
            num_cpus: 16,
            cpu_hist: vec![1.0, 2.5, 3.0, 0.0],
            ws_hist: vec![100.0, 110.5],
            priv_hist: vec![],
            io_r_hist: vec![0.0, 0.0, 1.0],
            io_w_hist: vec![2.0],
            thread_hist: vec![4.0, 4.0],
            handle_hist: vec![],
            total_io_read_mb: 12.5,
            total_io_write_mb: 3.25,
            flame,
        }
    }

    #[test]
    fn capture_round_trips() {
        let cap = sample_capture();
        let bytes = write_capture(&cap);
        let back = read_capture(&bytes).expect("round-trip valido");

        assert_eq!(back.argus_version, "0.1.0");
        assert_eq!(back.process_name, "fixture.exe");
        assert_eq!(back.pid, 1234);
        assert_eq!(back.num_cpus, 16);
        assert_eq!(back.cpu_hist, vec![1.0, 2.5, 3.0, 0.0]);
        assert_eq!(back.ws_hist, vec![100.0, 110.5]);
        assert!(back.priv_hist.is_empty());
        assert_eq!(back.io_r_hist, vec![0.0, 0.0, 1.0]);
        assert_eq!(back.total_io_read_mb, 12.5);
        assert_eq!(back.total_io_write_mb, 3.25);

        // Flame: stessi sample e stessa struttura.
        assert_eq!(back.flame.total_samples(), cap.flame.total_samples());
        assert_eq!(back.flame.node_count(), cap.flame.node_count());
        // Cammina fino a un nodo noto e confronta i conteggi.
        let walk = |g: &FlameGraph, path: &[&str]| -> Option<NodeId> {
            let mut cur = crate::aggregation::flame::ROOT;
            'outer: for &want in path {
                for &c in g.children_of(cur) {
                    if g.name_of(c) == want {
                        cur = c;
                        continue 'outer;
                    }
                }
                return None;
            }
            Some(cur)
        };
        let inner = walk(&back.flame, &["main", "compute", "inner"]).unwrap();
        assert_eq!(back.flame.total_of(inner), 2);
        assert_eq!(back.flame.own_of(inner), 2);
    }

    #[test]
    fn rejects_bad_magic_and_truncation() {
        assert!(read_capture(b"non e' un file argus").is_err());
        assert!(read_capture(&[]).is_err());

        let mut bytes = write_capture(&sample_capture());
        bytes.truncate(bytes.len() / 2);
        assert!(
            read_capture(&bytes).is_err(),
            "file troncato deve dare Err, non panic"
        );
    }
}
