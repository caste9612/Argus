// Niente console nera in release: Argus è un'app GUI.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use argus::app::ArgusApp;
use eframe::egui;
use tracing::info;
use tracing_appender::non_blocking::WorkerGuard;

fn main() -> eframe::Result {
    let _guard = init_logging();
    info!("Argus {} avviato", env!("CARGO_PKG_VERSION"));

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("Argus"),
        renderer: eframe::Renderer::Wgpu,
        vsync: true,
        // wgpu in auto (backend PRIMARY: Vulkan/DX12/…): sceglie il primo
        // disponibile sull'hardware. È la scelta più portabile — forzare un
        // singolo backend si è rivelato fragile su GPU molto recenti. Vedi
        // docs/03-tech-stack.md.
        ..Default::default()
    };

    // `--attach <pid>`: si collega subito al processo indicato e apre la tab Flame.
    let args: Vec<String> = std::env::args().collect();
    let initial_pid = args
        .iter()
        .position(|a| a == "--attach")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<u32>().ok());
    if let Some(pid) = initial_pid {
        info!("auto-attach da riga di comando: PID {pid}");
    }

    eframe::run_native(
        "Argus",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(egui::Visuals::dark());
            Ok(Box::new(ArgusApp::new(initial_pid)))
        }),
    )
}

/// Inizializza il logging su file in `%LOCALAPPDATA%\Argus\`. Se non possibile
/// (cartella non scrivibile), ripiega su stderr senza bloccare l'avvio.
///
/// Filtro di default: tutto a `warn`, solo Argus a `info` — così i log verbosi
/// di wgpu/eframe non inondano il file. Sovrascrivibile via `RUST_LOG`.
fn init_logging() -> Option<WorkerGuard> {
    use tracing_subscriber::{fmt, EnvFilter};

    // wgpu_hal emette warning Vulkan innocui (present mode, validation layer):
    // li silenziamo per tenere il log leggibile.
    let filter = || {
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("warn,argus=info,wgpu_hal=error"))
    };

    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let dir = std::path::Path::new(&local).join("Argus");
        if std::fs::create_dir_all(&dir).is_ok() {
            let appender = tracing_appender::rolling::daily(&dir, "argus.log");
            let (nb, guard) = tracing_appender::non_blocking(appender);
            fmt()
                .with_writer(nb)
                .with_ansi(false)
                .with_env_filter(filter())
                .init();
            return Some(guard);
        }
    }

    // Fallback: stderr, nessun guard da mantenere.
    fmt().with_env_filter(filter()).init();
    None
}
