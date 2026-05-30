//! Test d'integrazione **LIVE** della cattura ETW (Fase 2). Richiede privilegi
//! di **AMMINISTRATORE** (NT Kernel Logger), quindi è `#[ignore]`d: il normale
//! `cargo test` lo salta. Per eseguirlo, da un terminale **elevato**:
//!
//! ```text
//! cargo test --test etw_live -- --ignored --nocapture
//! ```
//!
//! Verifica end-to-end il path ETW: avvio sessione kernel + stack-walk +
//! consumer + parsing + filtro per PID, attaccandosi a un binario `fixture`
//! CPU-bound e controllando che arrivino stack reali. Prova anche a risolvere i
//! simboli del target vivo (best-effort).

use argus::aggregation::flame::FlameGraph;
use argus::capture::etw::EtwProfiler;
use argus::capture::process::open_for_symbols;
use argus::capture::symbols::SymbolResolver;
use std::process::Command;
use std::time::{Duration, Instant};

#[test]
#[ignore = "richiede privilegi di amministratore (ETW kernel logger)"]
fn captures_real_stacks_from_fixture() {
    // Fixture: 4 thread CPU-bound, 64 MB, per 8 s (resta vivo per la cattura).
    let mut child = Command::new(env!("CARGO_BIN_EXE_fixture"))
        .args(["4", "64", "8000"])
        .spawn()
        .expect("avvio del fixture");
    let pid = child.id();
    println!("== ETW LIVE == fixture avviato, PID {pid}");
    std::thread::sleep(Duration::from_millis(600)); // warmup: thread attivi

    let (tx, rx) = crossbeam_channel::bounded(16_384);
    let profiler = match EtwProfiler::start(pid, tx) {
        Ok(p) => p,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            panic!("EtwProfiler::start fallito (sei amministratore?): {e}");
        }
    };
    println!("sessione ETW avviata, raccolgo ~3 s di stack…");

    // Raccogli stack per ~3 secondi.
    let mut samples = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if let Ok(s) = rx.recv_timeout(Duration::from_millis(200)) {
            samples.push(s);
        }
    }

    // Risoluzione simboli del target (ancora vivo), best-effort.
    let mut resolved: Vec<String> = Vec::new();
    // Diagnostica ordine: estremi dello stack più profondo (raw, come da ETW).
    let mut order_hint = String::from("(nessuno stack profondo)");
    if let Ok(h) = open_for_symbols(pid) {
        if let Ok(mut r) = SymbolResolver::for_process(h.raw(), true) {
            for s in samples.iter().take(80) {
                for &addr in s.frames.iter().take(4) {
                    let name = r.resolve(addr);
                    if !name.starts_with("0x") {
                        resolved.push(name.to_string());
                    }
                }
            }
            let mut lines = Vec::new();
            for s in samples.iter().filter(|s| s.frames.len() >= 4).take(6) {
                let first = r.resolve(*s.frames.first().unwrap());
                let last = r.resolve(*s.frames.last().unwrap());
                lines.push(format!("[0]={first}  ||  [ultimo]={last}"));
            }
            if !lines.is_empty() {
                order_hint = lines.join("\n                ");
            }
        }
    }

    // Stop cattura + chiusura fixture.
    drop(profiler);
    let _ = child.kill();
    let _ = child.wait();

    // Costruisci un flame dagli indirizzi grezzi (root→leaf) per provare la pipe.
    let mut flame = FlameGraph::new();
    for s in &samples {
        let names: Vec<String> = s.frames.iter().rev().map(|a| format!("0x{a:x}")).collect();
        flame.add_stack(&names);
    }

    // --- Diagnostica leggibile ---
    let max_depth = samples.iter().map(|s| s.frames.len()).max().unwrap_or(0);
    let all_target = samples.iter().all(|s| s.pid == pid);
    println!("RISULTATO:");
    println!("  stack catturati ......... {}", samples.len());
    println!("  tutti del target (PID {pid})? {all_target}");
    println!("  profondità massima stack . {max_depth}");
    println!(
        "  nodi flame / sample ...... {} / {}",
        flame.node_count(),
        flame.total_samples()
    );
    resolved.sort();
    resolved.dedup();
    println!("  simboli risolti (esempi) . {}", resolved.len());
    for name in resolved.iter().take(12) {
        println!("      {name}");
    }
    // Se frames[ultimo] è l'entry-point del thread (BaseThreadInitThunk /
    // RtlUserThreadStart), allora ETW dà gli stack leaf-first e l'inversione
    // (.rev()) verso root→leaf è corretta.
    println!("  ORDINE STACK: {order_hint}");

    // --- Asserzioni: la pipe ETW funziona ---
    assert!(
        !samples.is_empty(),
        "nessuno stack catturato: stack-walk non attivo o provider PROFILE non abilitato"
    );
    assert!(
        all_target,
        "alcuni stack non appartengono al PID target: filtro errato"
    );
    assert!(flame.total_samples() > 0, "il flame graph è vuoto");
    println!("== ETW LIVE == OK ✅");
}
