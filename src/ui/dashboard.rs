//! Pannello centrale: KPI card + grafici time-series del processo collegato.

use super::kpi;
use crate::aggregation::diskstats::DiskStats;
use crate::aggregation::{Snapshot, Status};
use eframe::egui;
use parking_lot::Mutex;

// Palette (vedi docs/05-ui-design.md).
const BLUE: egui::Color32 = egui::Color32::from_rgb(124, 185, 255);
const PURPLE: egui::Color32 = egui::Color32::from_rgb(180, 154, 255);
const TEAL: egui::Color32 = egui::Color32::from_rgb(127, 224, 185);
const AMBER: egui::Color32 = egui::Color32::from_rgb(240, 198, 116);
const PINK: egui::Color32 = egui::Color32::from_rgb(255, 165, 224);

pub fn render(ui: &mut egui::Ui, snap: &Snapshot, disk: &Mutex<DiskStats>) {
    match &snap.status {
        Status::NotAttached | Status::Error(_) => placeholder(ui),
        Status::Running | Status::Exited | Status::Replay(_) => dashboard(ui, snap, disk),
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

fn dashboard(ui: &mut egui::Ui, snap: &Snapshot, disk: &Mutex<DiskStats>) {
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

            // --- Riga KPI ---
            ui.horizontal_wrapped(|ui| {
                kpi::card(
                    ui,
                    "CPU",
                    &format!("{:.1} %", snap.cpu),
                    kpi::cpu_color(snap.cpu),
                    "Tempo CPU (kernel+user) normalizzato su tutte le CPU logiche.",
                );
                kpi::card(
                    ui,
                    "RAM (working set)",
                    &format!("{:.1} MB", snap.working_set_mb),
                    BLUE,
                    "Memoria fisicamente residente in RAM. Oscillazioni = paging.",
                );
                kpi::card(
                    ui,
                    "RAM (private)",
                    &format!("{:.1} MB", snap.private_mb),
                    PURPLE,
                    "Memoria committed non condivisa. Crescita monotona = possibile leak.",
                );
                kpi::card(
                    ui,
                    "Thread",
                    &format!("{}", snap.threads),
                    AMBER,
                    "Numero di thread (campionato 1×/s).",
                );
                kpi::card(
                    ui,
                    "Handle",
                    &format!("{}", snap.handles),
                    PINK,
                    "Handle kernel aperti. Crescita monotona = handle leak.",
                );
                kpi::card(
                    ui,
                    "I/O lettura",
                    &format!("{:.2} MB/s", snap.io_read_mb_s),
                    TEAL,
                    "Throughput di lettura (disco + pipe + console).",
                );
                kpi::card(
                    ui,
                    "I/O scrittura",
                    &format!("{:.2} MB/s", snap.io_write_mb_s),
                    AMBER,
                    "Throughput di scrittura.",
                );
            });

            ui.add_space(10.0);

            kpi::chart(
                ui,
                "CPU  (%)",
                &snap.cpu_hist,
                160.0,
                kpi::cpu_color(snap.cpu),
                Some(100.0),
            );
            ui.add_space(8.0);

            kpi::chart_dual(
                ui,
                "Memoria  (MB)",
                ("working set", &snap.ws_hist, BLUE),
                ("private", &snap.priv_hist, PURPLE),
                160.0,
            );
            ui.add_space(8.0);

            kpi::chart_dual(
                ui,
                "I/O  (MB/s)",
                ("lettura", &snap.io_r_hist, TEAL),
                ("scrittura", &snap.io_w_hist, AMBER),
                140.0,
            );
            ui.add_space(8.0);

            ui.columns(2, |cols| {
                kpi::chart(
                    &mut cols[0],
                    "Thread",
                    &snap.thread_hist,
                    120.0,
                    AMBER,
                    None,
                );
                kpi::chart(&mut cols[1], "Handle", &snap.handle_hist, 120.0, PINK, None);
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
