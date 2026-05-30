//! Layer 5 — UI. Immediate-mode con egui.
//!
//! La UI è "pura": legge uno `Snapshot` immutabile + la lista processi, e
//! accumula i comandi da inviare al sampler in un `Vec<Command>` (out-param).
//! Nessun accesso diretto a Win32 o ai canali qui dentro.

mod dashboard;
mod kpi;
mod process_list;

use crate::aggregation::{Snapshot, Status};
use crate::capture::process::ProcessInfo;
use crate::capture::sampler::Command;
use eframe::egui;

/// Stato transitorio della UI (non condiviso col sampler).
#[derive(Default)]
pub struct State {
    pub search: String,
    pub selected_pid: Option<u32>,
}

/// Disegna l'intera UI per un frame. I comandi da eseguire vengono accodati in
/// `out`.
pub fn render(
    ctx: &egui::Context,
    state: &mut State,
    snap: &Snapshot,
    procs: &[ProcessInfo],
    out: &mut Vec<Command>,
) {
    top_bar(ctx, snap, out);
    process_list::render(ctx, state, procs, out);
    egui::CentralPanel::default().show(ctx, |ui| {
        dashboard::render(ui, snap);
    });
}

fn top_bar(ctx: &egui::Context, snap: &Snapshot, out: &mut Vec<Command>) {
    egui::TopBottomPanel::top("top").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.heading("Argus");
            ui.separator();
            ui.label(format!("CPU logiche: {}", snap.num_cpus));
            ui.separator();

            if snap.has_debug_privilege {
                ui.colored_label(egui::Color32::from_rgb(124, 217, 146), "🛡 elevato")
                    .on_hover_text("SeDebugPrivilege attivo — accesso esteso ai processi.");
            } else {
                ui.colored_label(egui::Color32::from_rgb(148, 148, 162), "utente")
                    .on_hover_text(
                        "Esecuzione utente: i processi di sistema o admin saranno \
                         inaccessibili. Rilancia come amministratore per accedervi.",
                    );
            }
            ui.separator();

            match &snap.status {
                Status::Running => {
                    if let Some(m) = &snap.attached {
                        ui.colored_label(
                            egui::Color32::from_rgb(124, 217, 146),
                            format!("● {} (PID {})", m.name, m.pid),
                        );
                        if ui.button("Scollega").clicked() {
                            out.push(Command::Detach);
                        }
                    }
                }
                Status::Exited => {
                    let label = snap
                        .attached
                        .as_ref()
                        .map(|m| format!("◌ {} terminato", m.name))
                        .unwrap_or_else(|| "◌ processo terminato".into());
                    ui.colored_label(egui::Color32::from_rgb(240, 198, 116), label);
                    if ui.button("Chiudi").clicked() {
                        out.push(Command::Detach);
                    }
                }
                Status::Error(msg) => {
                    ui.colored_label(egui::Color32::from_rgb(255, 107, 107), "⚠")
                        .on_hover_text(msg.clone());
                    ui.colored_label(egui::Color32::from_rgb(255, 107, 107), truncate(msg, 90));
                    if ui.button("OK").clicked() {
                        out.push(Command::Detach);
                    }
                }
                Status::NotAttached => {
                    ui.colored_label(egui::Color32::from_rgb(200, 200, 120), "○ non collegato");
                }
            }
        });
    });
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut t: String = s.chars().take(max).collect();
        t.push('…');
        t
    }
}
