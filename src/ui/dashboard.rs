//! Pannello centrale: KPI card + grafici time-series del processo collegato.

use super::kpi;
use crate::aggregation::diskstats::DiskStats;
use crate::aggregation::memstats::MemStats;
use crate::aggregation::{Snapshot, Status};
use eframe::egui;
use parking_lot::Mutex;

// Palette (vedi docs/05-ui-design.md).
const BLUE: egui::Color32 = egui::Color32::from_rgb(124, 185, 255);
const PURPLE: egui::Color32 = egui::Color32::from_rgb(180, 154, 255);
const TEAL: egui::Color32 = egui::Color32::from_rgb(127, 224, 185);
const AMBER: egui::Color32 = egui::Color32::from_rgb(240, 198, 116);
const PINK: egui::Color32 = egui::Color32::from_rgb(255, 165, 224);

pub fn render(ui: &mut egui::Ui, snap: &Snapshot, disk: &Mutex<DiskStats>, mem: &Mutex<MemStats>) {
    match &snap.status {
        Status::NotAttached | Status::Error(_) => placeholder(ui),
        Status::Running | Status::Exited | Status::Replay(_) => dashboard(ui, snap, disk, mem),
    }
}

fn mb(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

fn placeholder(ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(100.0);
        ui.heading("Nessun processo collegato");
        ui.add_space(8.0);
        ui.label("Seleziona un processo nella lista a sinistra e premi «Collega».");
        ui.add_space(4.0);
        ui.weak("Suggerimento: doppio click su un processo per collegarti subito.");
    });
}

fn dashboard(ui: &mut egui::Ui, snap: &Snapshot, disk: &Mutex<DiskStats>, mem: &Mutex<MemStats>) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if snap.status == Status::Exited {
                ui.colored_label(
                    AMBER,
                    "Il processo è terminato — i grafici mostrano l'ultimo stato registrato.",
                );
                ui.add_space(4.0);
            }

            // --- Hero CPU + griglia metriche compatte (ognuna con sparkline) ---
            kpi::hero(
                ui,
                "CPU",
                &format!("{:.1} %", snap.cpu),
                kpi::cpu_color(snap.cpu),
                "Tempo CPU (kernel+user) normalizzato su tutte le CPU logiche.",
                &snap.cpu_hist,
            );
            ui.add_space(10.0);
            metric_grid(ui, snap);

            ui.add_space(12.0);

            // Grafici principali su due colonne (meno scroll, più uso dello spazio).
            ui.columns(2, |cols| {
                kpi::chart_dual(
                    &mut cols[0],
                    "Memoria  (MB)",
                    ("working set", &snap.ws_hist, BLUE),
                    ("private", &snap.priv_hist, PURPLE),
                    150.0,
                );
                kpi::chart_dual(
                    &mut cols[1],
                    "I/O  (MB/s)",
                    ("lettura", &snap.io_r_hist, TEAL),
                    ("scrittura", &snap.io_w_hist, AMBER),
                    150.0,
                );
            });

            ui.add_space(8.0);
            ui.group(|ui| {
                ui.label(format!(
                    "I/O cumulativo — letti {:.1} MB · scritti {:.1} MB",
                    snap.total_io_read_mb, snap.total_io_write_mb
                ));
            });

            ui.add_space(8.0);
            disk_section(ui, disk);

            ui.add_space(8.0);
            mem_section(ui, mem);
        });
}

/// Griglia responsive di card metriche: il numero di colonne si adatta alla
/// larghezza (in righe bilanciate 6/3/2/1) e ogni card riempie la sua colonna,
/// così la riga si comporta come hero e grafici — niente spazio morto.
fn metric_grid(ui: &mut egui::Ui, snap: &Snapshot) {
    let cards = [
        (
            "RAM (working set)",
            format!("{:.1} MB", snap.working_set_mb),
            BLUE,
            "Memoria fisicamente residente in RAM. Oscillazioni = paging.",
            &snap.ws_hist,
        ),
        (
            "RAM (private)",
            format!("{:.1} MB", snap.private_mb),
            PURPLE,
            "Memoria committed non condivisa. Crescita monotona = possibile leak.",
            &snap.priv_hist,
        ),
        (
            "Thread",
            format!("{}", snap.threads),
            AMBER,
            "Numero di thread (campionato 1×/s).",
            &snap.thread_hist,
        ),
        (
            "Handle",
            format!("{}", snap.handles),
            PINK,
            "Handle kernel aperti. Crescita monotona = handle leak.",
            &snap.handle_hist,
        ),
        (
            "I/O lettura",
            format!("{:.2} MB/s", snap.io_read_mb_s),
            TEAL,
            "Throughput di lettura (disco + pipe + console).",
            &snap.io_r_hist,
        ),
        (
            "I/O scrittura",
            format!("{:.2} MB/s", snap.io_write_mb_s),
            AMBER,
            "Throughput di scrittura.",
            &snap.io_w_hist,
        ),
    ];
    let spacing = ui.spacing().item_spacing.x;
    let avail = ui.available_width();
    // Quante card ci stanno (target ~150px), poi arrotonda a un divisore di 6
    // per avere righe bilanciate (6, oppure 3+3, 2+2+2, …).
    let natural = (((avail + spacing) / (150.0 + spacing)).floor() as usize).max(1);
    let per_row = [6usize, 3, 2, 1]
        .into_iter()
        .find(|&d| d <= natural)
        .unwrap_or(1);
    for chunk in cards.chunks(per_row) {
        ui.columns(per_row, |cols| {
            for (i, c) in chunk.iter().enumerate() {
                kpi::metric_card(&mut cols[i], c.0, &c.1, c.2, c.3, c.4);
            }
        });
    }
}

/// Dettaglio memoria dagli eventi PageFault/VirtualAlloc ETW (filtrati sul
/// target): mostrato solo quando ci sono dati.
fn mem_section(ui: &mut egui::Ui, mem: &Mutex<MemStats>) {
    let m = mem.lock();
    if m.is_empty() {
        return;
    }
    ui.group(|ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Memoria (ETW)").strong());
            ui.weak("· del target durante la cattura");
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            kpi::card(
                ui,
                "Hard page fault",
                &format!("{}", m.hard_faults),
                PINK,
                &format!(
                    "Page-in da disco (fault costosi): {:.1} MB letti totali. \
                     Alti = il working set non sta in RAM (paging).",
                    mb(m.hard_fault_bytes)
                ),
            );
            kpi::card(
                ui,
                "VirtualAlloc",
                &format!("{:.1} MB", mb(m.valloc_bytes)),
                PURPLE,
                &format!(
                    "{} riserve/commit di memoria virtuale (granularità di pagina, \
                     non HeapAlloc).",
                    m.valloc_count
                ),
            );
            let net = m.net_alloc_bytes();
            kpi::card(
                ui,
                "Saldo netto",
                &format!("{:+.1} MB", net as f64 / (1024.0 * 1024.0)),
                if net > 0 { AMBER } else { TEAL },
                "VirtualAlloc − VirtualFree: positivo e crescente = la memoria \
                 virtuale riservata sale (possibile crescita/leak).",
            );
        });
    });
}

/// Dettaglio disco dagli eventi DiskIo ETW (di sistema): mostrato solo quando
/// ci sono dati (cattura ETW attiva e disco usato).
fn disk_section(ui: &mut egui::Ui, disk: &Mutex<DiskStats>) {
    let d = disk.lock();
    if d.is_empty() {
        return;
    }
    ui.group(|ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Disco fisico (ETW)").strong());
            ui.weak("· attività di sistema durante la cattura, non solo del target");
        });
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            kpi::card(
                ui,
                "Disco lettura",
                &format!("{:.1} MB", mb(d.read.bytes)),
                TEAL,
                &format!(
                    "{} operazioni · {} KB/op in media",
                    d.read.ops,
                    d.read.avg_size() / 1024
                ),
            );
            kpi::card(
                ui,
                "Disco scrittura",
                &format!("{:.1} MB", mb(d.write.bytes)),
                AMBER,
                &format!(
                    "{} operazioni · {} KB/op in media",
                    d.write.ops,
                    d.write.avg_size() / 1024
                ),
            );
        });
        ui.add_space(4.0);
        // Latenza: tick QPC grezzi → ms (clock del trace = QPC). p50/p99/max.
        let qpf = crate::util::win::qpc_frequency().max(1) as f64;
        let to_ms = |raw: u64| raw as f64 / qpf * 1000.0;
        ui.label(
            egui::RichText::new(format!(
                "Latenza per operazione:  mediana {:.2} ms  ·  p99 {:.2} ms  ·  max {:.2} ms",
                to_ms(d.p50_response_raw()),
                to_ms(d.p99_response_raw()),
                to_ms(d.max_response_raw())
            ))
            .small(),
        )
        .on_hover_text(
            "Tempo di risposta delle operazioni di disco, dal clock del trace (QPC). \
             p99 alto = code di latenza occasionali (disco sotto pressione).",
        );
        ui.add_space(2.0);
        for (disk_n, r, w) in d.disks_by_bytes().into_iter().take(6) {
            ui.label(
                egui::RichText::new(format!(
                    "Disco {disk_n}:  letti {:.1} MB ({} op)  ·  scritti {:.1} MB ({} op)",
                    mb(r.bytes),
                    r.ops,
                    mb(w.bytes),
                    w.ops
                ))
                .small(),
            );
        }
    });
}
