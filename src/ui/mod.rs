//! Layer 5 — UI. Immediate-mode con egui.
//!
//! La UI è "pura": legge uno `Snapshot` immutabile + la lista processi, e
//! accumula i comandi da inviare al sampler in un `Vec<Command>` (out-param).
//! Nessun accesso diretto a Win32 o ai canali qui dentro.

mod dashboard;
mod diff_view;
mod flame;
mod kpi;
mod process_list;

use crate::aggregation::flame::{FlameGraph, NodeId};
use crate::aggregation::{Snapshot, Status};
use crate::capture::process::ProcessInfo;
use crate::capture::sampler::Command;
use crate::diff::DiffSummary;
use crate::export::ExportKind;
use eframe::egui;
use parking_lot::Mutex;
use std::path::PathBuf;

/// Tab del pannello centrale.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Dashboard,
    Flame,
    Diff,
}

/// Criterio di ordinamento secondario della lista processi (il raggruppamento
/// "utente prima" è sempre primario).
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    #[default]
    Cpu,
    Mem,
    Name,
}

/// Stato transitorio della UI (non condiviso col sampler).
#[derive(Default)]
pub struct State {
    pub search: String,
    pub selected_pid: Option<u32>,
    pub sort_key: SortKey,
    pub hide_system: bool,
    pub tab: Tab,
    /// Nodo su cui è zoomato il flame graph (ROOT = vista intera).
    pub flame_focus: NodeId,
    pub flame_search: String,
}

/// Disegna l'intera UI per un frame. I comandi da eseguire vengono accodati in
/// `out`. `flame` è l'albero condiviso con l'aggregatore (letto sotto lock).
#[allow(clippy::too_many_arguments)] // UI di frame: tanti dati di sola lettura
pub fn render(
    ctx: &egui::Context,
    state: &mut State,
    snap: &Snapshot,
    procs: &[ProcessInfo],
    flame: &Mutex<FlameGraph>,
    captures: &[PathBuf],
    diff: Option<&DiffSummary>,
    out: &mut Vec<Command>,
) {
    top_bar(ctx, snap, captures, out);
    process_list::render(ctx, state, procs, out);
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut state.tab, Tab::Dashboard, "📊 Metriche");
            ui.selectable_value(&mut state.tab, Tab::Flame, "🔥 Flame graph");
            ui.selectable_value(&mut state.tab, Tab::Diff, "⇄ Diff");
        });
        ui.separator();
        match state.tab {
            Tab::Dashboard => dashboard::render(ui, snap),
            Tab::Flame => flame::render(
                ui,
                snap,
                flame,
                &mut state.flame_focus,
                &mut state.flame_search,
            ),
            Tab::Diff => diff_view::render(ui, diff),
        }
    });
}

fn top_bar(ctx: &egui::Context, snap: &Snapshot, captures: &[PathBuf], out: &mut Vec<Command>) {
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
                Status::Replay(label) => {
                    ui.colored_label(
                        egui::Color32::from_rgb(124, 185, 255),
                        format!("▷ replay: {label}"),
                    );
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

            // --- Controlli sessione (Fase 4): salva / apri ---
            ui.separator();
            let has_data = !matches!(snap.status, Status::NotAttached);
            if ui
                .add_enabled(has_data, egui::Button::new("💾 Salva"))
                .on_hover_text("Salva la sessione corrente in un file .argus")
                .clicked()
            {
                out.push(Command::SaveCapture);
            }
            ui.menu_button("📂 Apri", |ui| {
                out.push(Command::RefreshCaptures);
                if captures.is_empty() {
                    ui.label("Nessuna sessione salvata.");
                }
                for path in captures {
                    let label = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("(senza nome)");
                    if ui.button(label).clicked() {
                        out.push(Command::OpenCapture(path.clone()));
                        ui.close_menu();
                    }
                }
            });
            ui.add_enabled_ui(has_data, |ui| {
                ui.menu_button("⬇ Esporta", |ui| {
                    if ui.button("CSV (metriche)").clicked() {
                        out.push(Command::Export(ExportKind::Csv));
                        ui.close_menu();
                    }
                    if ui.button("Folded stacks (speedscope)").clicked() {
                        out.push(Command::Export(ExportKind::Folded));
                        ui.close_menu();
                    }
                    if ui.button("SVG (flame graph)").clicked() {
                        out.push(Command::Export(ExportKind::Svg));
                        ui.close_menu();
                    }
                });
            });
            ui.add_enabled_ui(has_data, |ui| {
                ui.menu_button("⇄ Confronta", |ui| {
                    out.push(Command::RefreshCaptures);
                    ui.label("Baseline da confrontare con la sessione corrente:");
                    if captures.is_empty() {
                        ui.weak("Nessuna sessione salvata.");
                    }
                    for path in captures {
                        let label = path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("(senza nome)");
                        if ui.button(label).clicked() {
                            out.push(Command::DiffCapture(path.clone()));
                            ui.close_menu();
                        }
                    }
                });
            });
        });

        // Seconda riga: messaggio transitorio (salvataggio/caricamento).
        if let Some(msg) = &snap.notice {
            ui.colored_label(egui::Color32::from_rgb(124, 185, 255), truncate(msg, 120));
        }
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
