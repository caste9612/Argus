//! Misura dell'**overhead** introdotto da Argus sul processo osservato — il
//! claim di testa di `01-vision` ("< 1% sul target"). Metodo: a parità di
//! **lavoro fisso** (fixture in modalità `bench`), si confronta il tempo di
//! esecuzione da solo vs mentre la sessione ETW kernel (sampling ~1 kHz +
//! context switch) è attiva. La dilatazione percentuale è l'overhead.
//!
//! Richiede privilegi di **amministratore** (NT Kernel Logger) → `#[ignore]`.
//! Va eseguito **in release** (il debug falsa i tempi) da un terminale elevato:
//!
//! ```text
//! cargo test --release --test overhead -- --ignored --nocapture
//! ```
//!
//! Se non elevato, il test stampa SKIP e passa (non misura). Le soglie di
//! `assert` sono volutamente larghe: un loop CPU-bound stretto è il **caso
//! peggiore** per il sampling (massima frequenza di interruzioni); il numero
//! reale viene stampato e riportato in docs/06-reliability.md.

use argus::capture::etw::EtwProfiler;
use std::collections::HashSet;
use std::process::Command;
use std::time::Duration;

/// Esegue il fixture in modalità bench (`miter` milioni di iterazioni) e ritorna
/// il tempo auto-misurato in ms (indipendente dal costo di spawn del processo).
fn bench_once(miter: u32) -> f64 {
    let out = Command::new(env!("CARGO_BIN_EXE_fixture"))
        .args(["bench", &miter.to_string()])
        .output()
        .expect("esecuzione del fixture bench");
    let stdout = String::from_utf8_lossy(&out.stdout);
    parse_elapsed(&stdout).unwrap_or_else(|| panic!("output bench inatteso: {stdout:?}"))
}

/// Estrae `elapsed_ms=<f>` dalla riga `BENCH …`.
fn parse_elapsed(s: &str) -> Option<f64> {
    let tok = s
        .split_whitespace()
        .find_map(|t| t.strip_prefix("elapsed_ms="))?;
    tok.parse().ok()
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    v[v.len() / 2]
}

/// Numero di milioni di iterazioni per run. ~2 s in release (≈2 ms/Miter sulla
/// macchina di sviluppo): abbastanza lungo da campionare a fondo (~2000 sample a
/// 1 kHz), abbastanza corto da fare più ripetizioni.
const MITER: u32 = 1000;
const RUNS: usize = 5;

#[test]
#[ignore = "richiede privilegi di amministratore + esecuzione in release"]
fn etw_profiling_overhead() {
    // Baseline: lavoro fisso da solo (mediana di RUNS run).
    let baseline = median((0..RUNS).map(|_| bench_once(MITER)).collect());
    println!("== OVERHEAD == baseline (no ETW) mediana = {baseline:.1} ms");

    // Avvia la sessione ETW kernel (sampling system-wide attivo). Il filtro per
    // PID cambia solo quali eventi si CONSERVANO, non il costo del sampling, che
    // ricade su tutte le CPU — quindi anche sul fixture.
    let (tx, rx) = crossbeam_channel::bounded(16_384);
    let profiler = match EtwProfiler::start(std::process::id(), HashSet::new(), tx) {
        Ok(p) => p,
        Err(e) => {
            println!("SKIP: ETW non disponibile (serve amministratore): {e}");
            return; // non elevato: niente misura, ma il test passa
        }
    };
    // Drena gli eventi per non saturare il canale (come fa l'aggregatore vero).
    let drain = std::thread::spawn(
        move || {
            while rx.recv_timeout(Duration::from_millis(200)).is_ok() {}
        },
    );

    let with_etw = median((0..RUNS).map(|_| bench_once(MITER)).collect());
    drop(profiler); // ferma ETW → consumer esce → canale chiuso → drain esce
    let _ = drain.join();

    let overhead = (with_etw - baseline) / baseline * 100.0;
    println!("== OVERHEAD == con ETW mediana = {with_etw:.1} ms");
    println!("== OVERHEAD == overhead = {overhead:.2}% (target <1% su carichi reali)");

    // Guardia di affidabilità: un overhead nettamente negativo significa che il
    // "con ETW" è andato più veloce del baseline — impossibile se non per un
    // fattore ambientale (la macchina è andata in sospensione tra i due batch, o
    // il CPU ha cambiato frequenza/turbo). In quel caso la misura non è valida.
    if overhead < -5.0 {
        println!(
            "== OVERHEAD == ⚠ MISURA NON AFFIDABILE: overhead negativo ({overhead:.1}%). \
             Probabile sospensione del sistema o frequency scaling durante il test. \
             Riesegui senza lasciare sospendere la macchina."
        );
        return; // non assertare su una misura corrotta
    }

    // Soglia larga: il caso peggiore (loop stretto) può superare l'1%. Verifica
    // solo che non ci sia un'esplosione (bug: busy-loop nel consumer, ecc.).
    assert!(
        overhead < 25.0,
        "overhead {overhead:.2}% troppo alto: regressione?"
    );
}
