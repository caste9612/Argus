//! Carico di lavoro deterministico per i test d'integrazione di Argus.
//!
//! Due modalità:
//! - `fixture [threads] [alloc_mb] [hold_ms]` (default 4, 64, 3000): alloca e
//!   "tocca" la memoria, poi brucia CPU su N thread per `hold_ms`. Per i test
//!   che osservano un processo vivo (flame, timeline).
//! - `fixture bench [Miter]` (default 3000 milioni): lavoro **fisso** (non a
//!   tempo) su un solo thread, stampa `BENCH elapsed_ms=… checksum=…`. Serve a
//!   misurare l'overhead di Argus come dilatazione del tempo a parità di lavoro
//!   (vedi `tests/overhead.rs` e docs/06-reliability.md).

use std::time::{Duration, Instant};

/// Un passo di lavoro non ottimizzabile via (LCG + black_box).
#[inline(always)]
fn step(x: u64) -> u64 {
    let y = x.wrapping_mul(6364136223846793005).wrapping_add(1);
    std::hint::black_box(y)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Modalità bench: lavoro fisso, autotemporizzato, single-thread.
    if args.get(1).map(String::as_str) == Some("bench") {
        let miter: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(3000);
        let iters = miter.saturating_mul(1_000_000);
        let start = Instant::now();
        let mut x: u64 = 0x9e3779b97f4a7c15;
        for _ in 0..iters {
            x = step(x);
        }
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        // checksum stampato così il lavoro non può essere eliminato dall'ottimizzatore.
        println!("BENCH elapsed_ms={ms:.3} checksum={x}");
        return;
    }

    let threads: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(4);
    let alloc_mb: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(64);
    let hold_ms: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(3000);

    // Alloca e scrive una volta per pagina (4 KB) per renderla residente in RAM.
    let mut block = vec![0u8; alloc_mb * 1024 * 1024];
    let mut i = 0;
    while i < block.len() {
        block[i] = (i & 0xff) as u8;
        i += 4096;
    }

    // Thread che bruciano CPU per hold_ms con lavoro non ottimizzabile via.
    let mut handles = Vec::with_capacity(threads);
    for _ in 0..threads {
        handles.push(std::thread::spawn(move || {
            let start = Instant::now();
            let mut x: u64 = 0x9e3779b97f4a7c15;
            while start.elapsed() < Duration::from_millis(hold_ms) {
                for _ in 0..10_000 {
                    x = step(x);
                }
            }
        }));
    }

    // Churn di allocazioni *durante* l'esecuzione: ogni ~150 ms riserva un blocco
    // grande (16 MB → va direttamente in VirtualAlloc, non nei pool dell'heap), lo
    // tocca e lo libera (VirtualFree). Dà attività di memoria osservabile mentre un
    // profiler è attaccato (così la cattura ETW non perde tutte le allocazioni).
    let churn_start = Instant::now();
    while churn_start.elapsed() < Duration::from_millis(hold_ms) {
        let mut chunk = vec![0u8; 16 * 1024 * 1024];
        let mut j = 0;
        while j < chunk.len() {
            chunk[j] = (j & 0xff) as u8;
            j += 4096;
        }
        std::hint::black_box(chunk.as_ptr());
        drop(chunk);
        std::thread::sleep(Duration::from_millis(150));
    }

    for h in handles {
        let _ = h.join();
    }

    // Mantiene viva l'allocazione fino alla fine.
    std::hint::black_box(block.len());
}
