//! Pannello sinistro: lista processi con ricerca e attach.

use super::State;
use crate::capture::process::ProcessInfo;
use crate::capture::sampler::Command;
use eframe::egui;

pub fn render(
    ctx: &egui::Context,
    state: &mut State,
    procs: &[ProcessInfo],
    out: &mut Vec<Command>,
) {
    egui::SidePanel::left("process_list")
        .resizable(true)
        .default_width(320.0)
        .min_width(240.0)
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
                ui.label(format!("{} processi", procs.len()));
                if ui.button("⟳ aggiorna").clicked() {
                    out.push(Command::RefreshProcesses);
                }
            });
            ui.separator();

            let query = state.search.to_lowercase();
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for p in procs {
                        if !query.is_empty() && !p.name.to_lowercase().contains(&query) {
                            continue;
                        }
                        let selected = state.selected_pid == Some(p.pid);
                        let label = format!("{}   ·   PID {}   ·   {} thr", p.name, p.pid, p.threads);
                        let resp = ui.selectable_label(selected, label);
                        if resp.clicked() {
                            state.selected_pid = Some(p.pid);
                        }
                        if resp.double_clicked() {
                            state.selected_pid = Some(p.pid);
                            out.push(Command::Attach(p.pid));
                        }
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
