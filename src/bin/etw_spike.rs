//! Spike ETW di Fase 2 (vedi `docs/10-etw-spike.md`).
//!
//! Uso, **da un terminale amministratore**:
//! ```text
//!   cargo run --features etw --bin etw_spike -- <pid>
//! ```
//!
//! Avvia una sessione ETW kernel col provider profile, imposta l'intervallo di
//! campionamento, attende qualche secondo e la ferma (RAII). Stampa l'esito.
//! Senza privilegi admin fallisce con un messaggio chiaro (no panic) — è il
//! primo milestone di validazione del control path dello spike.

use argus::capture::etw::KernelTraceSession;
use std::time::Duration;

fn main() {
    let pid: u32 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    // Logging su stderr utile durante lo spike (la sessione logga via tracing).
    tracing_subscriber::fmt()
        .with_env_filter("argus=debug")
        .with_writer(std::io::stderr)
        .try_init()
        .ok();

    println!("etw_spike: avvio sessione ETW kernel (target PID {pid})...");
    match KernelTraceSession::start() {
        Ok(_session) => {
            println!("OK: sessione avviata. Attesa 3 s (consumer = prossimo step)...");
            std::thread::sleep(Duration::from_secs(3));
            println!("Stop sessione (RAII a fine scope).");
            // `_session` viene droppato qui → ControlTraceW(EVENT_TRACE_CONTROL_STOP).
        }
        Err(e) => {
            eprintln!("Sessione NON avviata: {e}");
            eprintln!("Suggerimento: esegui da un terminale amministratore.");
            std::process::exit(1);
        }
    }
}
