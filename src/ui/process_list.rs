//! Pannello sinistro: lista processi con ricerca, ordinamento e attach.
//!
//! I processi dell'utente (stessa sessione di Argus) vengono sempre mostrati per
//! primi; all'interno di ciascun gruppo si ordina per il criterio scelto
//! (CPU/RAM/Nome). Ordinamento e filtro sono lato UI: istantanei, costo
//! trascurabile su qualche centinaio di righe.

use super::{SortKey, State};
use crate::capture::process::ProcessInfo;
use crate::capture::sampler::Command;
use eframe::egui;
use std::cmp::Ordering;

pub fn render(
    ctx: &egui::Context,
    state: &mut State,
    procs: &[ProcessInfo],
    out: &mut Vec<Command>,
) {
    egui::SidePanel::left("process_list")
        .resizable(true)
        .default_width(340.0)
        .min_width(260.0)
        .show(ctx, |ui| {
            ui.add_space(4.0);
            ui.heading("Processi");

            ui.horizontal(|ui| {
                ui.label("🔍");
                ui.add(
                    egui::TextEdit::singleline(&mut state.search)
                        .hint_text("filtra per nome…")
                        .desired_width(f32::INFINITY),
                );
            });

            ui.horizontal(|ui| {
                ui.label("Ordina:");
                ui.selectable_value(&mut state.sort_key, SortKey::Cpu, "CPU");
                ui.selectable_value(&mut state.sort_key, SortKey::Mem, "RAM");
                ui.selectable_value(&mut state.sort_key, SortKey::Name, "Nome");
                if ui.button("⟳").on_hover_text("Aggiorna lista").clicked() {
                    out.push(Command::RefreshProcesses);
                }
            });
            ui.checkbox(&mut state.hide_system, "Nascondi processi di sistema");

            // Filtro + ordinamento.
            let query = state.search.to_lowercase();
            let mut rows: Vec<&ProcessInfo> = procs
                .iter()
                .filter(|p| query.is_empty() || p.name.to_lowercase().contains(&query))
                .filter(|p| !state.hide_system || p.is_user)
                .collect();
            rows.sort_by(|a, b| sort_rows(a, b, state.sort_key));

            ui.label(format!("{} di {} processi", rows.len(), procs.len()));
            ui.separator();

            let mut header_shown = (false, false); // (utente, sistema)
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for p in rows {
                        // Intestazioni di gruppo.
                        if p.is_user && !header_shown.0 {
                            header_shown.0 = true;
                            ui.label(egui::RichText::new("UTENTE").small().weak());
                        } else if !p.is_user && !header_shown.1 {
                            header_shown.1 = true;
                            ui.add_space(4.0);
                            ui.label(egui::RichText::new("SISTEMA").small().weak());
                        }
                        row(ui, state, p, out);
                    }
                });

            ui.separator();
            ui.horizontal(|ui| {
                let enabled = state.selected_pid.is_some();
                if ui
                    .add_enabled(enabled, egui::Button::new("Collega"))
                    .clicked()
                {
                    if let Some(pid) = state.selected_pid {
                        out.push(Command::Attach(pid));
                    }
                }
                ui.weak("doppio click = collega");
            });
            ui.add_space(4.0);
        });
}

fn row(ui: &mut egui::Ui, state: &mut State, p: &ProcessInfo, out: &mut Vec<Command>) {
    let selected = state.selected_pid == Some(p.pid);
    let text = format!(
        "{}   ·   {:.0}%   ·   {:.0} MB",
        p.name, p.cpu_percent, p.working_set_mb
    );
    // I processi di sistema sono attenuati per dare risalto a quelli dell'utente.
    let rich = if p.is_user {
        egui::RichText::new(text)
    } else {
        egui::RichText::new(text).weak()
    };

    let resp = ui.selectable_label(selected, rich).on_hover_text(format!(
        "PID {} · sessione {} · {} thread\nCPU {:.1}% · working set {:.1} MB",
        p.pid, p.session_id, p.threads, p.cpu_percent, p.working_set_mb
    ));
    if resp.clicked() {
        state.selected_pid = Some(p.pid);
    }
    if resp.double_clicked() {
        state.selected_pid = Some(p.pid);
        out.push(Command::Attach(p.pid));
    }
}

/// Ordine: utente prima, poi per il criterio scelto (decrescente per le risorse).
fn sort_rows(a: &ProcessInfo, b: &ProcessInfo, key: SortKey) -> Ordering {
    b.is_user
        .cmp(&a.is_user)
        .then_with(|| match key {
            SortKey::Cpu => desc(a.cpu_percent, b.cpu_percent),
            SortKey::Mem => desc(a.working_set_mb, b.working_set_mb),
            SortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        })
        // A parità, la RAM decrescente come spareggio stabile.
        .then_with(|| desc(a.working_set_mb, b.working_set_mb))
}

#[inline]
fn desc(a: f32, b: f32) -> Ordering {
    b.partial_cmp(&a).unwrap_or(Ordering::Equal)
}
