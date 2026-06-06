//! Test d'integrazione **LIVE** della cattura ETW (Fase 2). Richiede privilegi
//! di **AMMINISTRATORE** (NT Kernel Logger), quindi è `#[ignore]`d: il normale
//! `cargo test` lo salta. Per eseguirlo, da un terminale **elevato**:
//!
//! ```text
//! cargo test --test etw_live -- --ignored --nocapture
//! ```
//!
//! Verifica end-to-end il path ETW (Fase 2 + 3): avvio sessione kernel +
//! stack-walk + CSwitch + consumer + parsing + filtri, attaccandosi a un binario
//! `fixture` CPU-bound. Controlla che arrivino stack reali (flame) e context
//! switch (timeline), e prova a risolvere i simboli del target vivo (best-effort).

use argus::aggregation::diskstats::DiskStats;
use argus::aggregation::flame::FlameGraph;
use argus::aggregation::memstats::MemStats;
use argus::aggregation::timeline::ThreadTimeline;
use argus::capture::etw::{EtwEvent, EtwProfiler};
use argus::capture::process::{open_for_symbols, thread_ids};
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

    let tids: std::collections::HashSet<u32> = thread_ids(pid).into_iter().collect();
    println!("thread del target: {}", tids.len());
    let mut timeline = ThreadTimeline::new();
    timeline.set_tracked(tids.clone()); // solo i thread del target
    let (tx, rx) = crossbeam_channel::bounded(16_384);
    let profiler = match EtwProfiler::start(pid, tids, tx) {
        Ok(p) => p,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            panic!("EtwProfiler::start fallito (sei amministratore?): {e}");
        }
    };
    println!("sessione ETW avviata, raccolgo ~3 s di eventi…");

    // Genera attività disco da validare (scrive e sincronizza un file temporaneo),
    // in un thread a parte così gira durante la finestra di cattura.
    let disk_work = std::thread::spawn(|| {
        use std::io::Write;
        let path = std::env::temp_dir().join("argus_diskio_probe.bin");
        if let Ok(mut f) = std::fs::File::create(&path) {
            let buf = vec![0xABu8; 1024 * 1024];
            for _ in 0..16 {
                let _ = f.write_all(&buf);
            }
            let _ = f.sync_all(); // forza il flush su disco → eventi DiskIo write
        }
        let _ = std::fs::remove_file(&path);
    });

    // Raccogli ~3 s: gli stack vanno in `samples`, i context-switch costruiscono
    // la timeline in tempo reale, le operazioni di disco in `diskstats`.
    let mut samples = Vec::new();
    let mut switch_count = 0usize;
    let mut diskstats = DiskStats::new();
    let mut memstats = MemStats::new();
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if let Ok(ev) = rx.recv_timeout(Duration::from_millis(200)) {
            match ev {
                EtwEvent::Stack(s) => samples.push(s),
                EtwEvent::Switch(sw) => {
                    switch_count += 1;
                    timeline.on_cswitch(
                        sw.timestamp,
                        sw.new_tid,
                        sw.old_tid,
                        sw.old_state,
                        sw.old_wait_reason,
                    );
                }
                EtwEvent::Disk(ev) => diskstats.on_event(&ev),
                EtwEvent::Mem(ev) => memstats.on_event(&ev),
            }
        }
    }
    let _ = disk_work.join();

    // Risoluzione simboli del target (ancora vivo) e costruzione del flame come
    // fa l'app vera (resolver → root→leaf), così si vede il merge per-funzione.
    let mut flame = FlameGraph::new();
    let mut resolved: Vec<String> = Vec::new();
    let mut order_hint = String::from("(nessuno stack profondo)");
    let resolver = open_for_symbols(pid)
        .ok()
        .and_then(|h| SymbolResolver::for_process(h.raw(), true).ok());
    if let Some(mut r) = resolver {
        for s in &samples {
            let names: Vec<_> = s.frames.iter().rev().map(|&a| r.resolve(a)).collect();
            flame.add_stack(&names);
        }
        for s in samples.iter().take(120) {
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
    } else {
        // Senza resolver: flame da indirizzi grezzi (graceful degradation).
        for s in &samples {
            let names: Vec<String> = s.frames.iter().rev().map(|a| format!("0x{a:x}")).collect();
            flame.add_stack(&names);
        }
    }

    // Stop cattura + chiusura fixture.
    drop(profiler);
    let _ = child.kill();
    let _ = child.wait();

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
    let (t0, t1) = timeline.span();
    println!(
        "  context switch ........... {switch_count} (timeline: {} thread, span {} tick)",
        timeline.thread_count(),
        t1.saturating_sub(t0)
    );
    // Disco (di sistema): validazione live del layout DiskIo. Valori plausibili
    // (dimensioni multiple di 512/4K, MB sensati) confermano il parser.
    let mb = |b: u64| b as f64 / (1024.0 * 1024.0);
    println!(
        "  disco lettura ............ {:.2} MB in {} op ({} B/op)",
        mb(diskstats.read.bytes),
        diskstats.read.ops,
        diskstats.read.avg_size()
    );
    println!(
        "  disco scrittura .......... {:.2} MB in {} op ({} B/op)",
        mb(diskstats.write.bytes),
        diskstats.write.ops,
        diskstats.write.avg_size()
    );
    let qpf = argus::util::win::qpc_frequency().max(1) as f64;
    let to_ms = |raw: u64| raw as f64 / qpf * 1000.0;
    println!(
        "  disco latenza ms ......... p50 {:.3} · p99 {:.3} · max {:.3} (avg raw {})",
        to_ms(diskstats.p50_response_raw()),
        to_ms(diskstats.p99_response_raw()),
        to_ms(diskstats.max_response_raw()),
        diskstats.avg_response_raw()
    );
    for (d, r, w) in diskstats.disks_by_bytes().into_iter().take(4) {
        println!(
            "      disco {d}: R {:.2} MB ({} op) · W {:.2} MB ({} op)",
            mb(r.bytes),
            r.ops,
            mb(w.bytes),
            w.ops
        );
    }
    // Memoria (target): validazione live del layout PageFault/VirtualAlloc. Il
    // fixture alloca 64 MB → ci aspettiamo VirtualAlloc del target.
    println!(
        "  hard page fault .......... {} ({:.2} MB letti)",
        memstats.hard_faults,
        mb(memstats.hard_fault_bytes)
    );
    println!(
        "  VirtualAlloc ............. {} op, {:.2} MB · VirtualFree {} op, {:.2} MB · netto {:.2} MB",
        memstats.valloc_count,
        mb(memstats.valloc_bytes),
        memstats.vfree_count,
        mb(memstats.vfree_bytes),
        memstats.net_alloc_bytes() as f64 / (1024.0 * 1024.0)
    );

    // --- Asserzioni: la pipe ETW funziona (flame + timeline) ---
    assert!(
        !samples.is_empty(),
        "nessuno stack catturato: stack-walk non attivo o provider PROFILE non abilitato"
    );
    assert!(
        all_target,
        "alcuni stack non appartengono al PID target: filtro errato"
    );
    assert!(flame.total_samples() > 0, "il flame graph è vuoto");
    assert!(
        switch_count > 0,
        "nessun context switch: provider CSWITCH non attivo?"
    );
    assert!(
        timeline.thread_count() > 0,
        "la timeline non ha intervalli: ricostruzione errata?"
    );
    println!("== ETW LIVE == OK ✅");
}
