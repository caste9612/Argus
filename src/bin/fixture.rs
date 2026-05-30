//! Carico di lavoro deterministico per i test d'integrazione di Argus.
//!
//! Uso: `fixture [threads] [alloc_mb] [hold_ms]` (default 4, 64, 3000).
//! Alloca e "tocca" la memoria (così entra nel working set), poi brucia CPU su
//! N thread per `hold_ms`. Comportamento prevedibile che i test possono misurare.

use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
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
                    x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
                    std::hint::black_box(x);
                }
            }
        }));
    }
    for h in handles {
        let _ = h.join();
    }

    // Mantiene viva l'allocazione fino alla fine.
    std::hint::black_box(block.len());
}
