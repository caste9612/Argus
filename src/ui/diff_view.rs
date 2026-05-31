//! Tab "Diff": confronto sovrapposto di due sessioni (Fase 4).
//!
//! Mostra, per ogni metrica, le serie A (baseline) e B (corrente) sovrapposte,
//! con media e picco; sotto, le funzioni che cambiano di più in self-time.

use crate::diff::DiffSummary;
use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints};

const COL_A: egui::Color32 = egui::Color32::from_rgb(150, 150, 165); // baseline (grigio)
const COL_B: egui::Color32 = egui::Color32::from_rgb(124, 185, 255); // corrente (blu)
const UP: egui::Color32 = egui::Color32::from_rgb(255, 107, 107); // in aumento
const DOWN: egui::Color32 = egui::Color32::from_rgb(124, 217, 146); // in calo

pub fn render(ui: &mut egui::Ui, diff: Option<&DiffSummary>) {
    let Some(d) = diff else {
        ui.add_space(60.0);
        ui.vertical_centered(|ui| {
            ui.label(
                "Nessun confronto attivo. Usa «Confronta» nella barra in alto per \
                 scegliere una baseline da confrontare con la sessione corrente.",
            );
        });
        return;
    };

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.colored_label(COL_A, format!("● A · baseline: {}", d.label_a));
                ui.separator();
                ui.colored_label(COL_B, format!("● B · corrente: {}", d.label_b));
            });
            ui.separator();

            for s in &d.series {
                ui.label(egui::RichText::new(&s.label).strong());
                ui.label(
                    egui::RichText::new(format!(
                        "media {:.1} → {:.1}   ·   picco {:.1} → {:.1}",
                        s.avg_a, s.avg_b, s.peak_a, s.peak_b
                    ))
                    .small()
                    .weak(),
                );
                let la = Line::new(idx_points(&s.a)).color(COL_A).width(1.4);
                let lb = Line::new(idx_points(&s.b)).color(COL_B).width(1.8);
                Plot::new(format!("diff_{}", s.label))
                    .height(100.0)
                    .show_axes([false, true])
                    .show_grid([false, true])
                    .allow_drag(false)
                    .allow_zoom(false)
                    .allow_scroll(false)
                    .include_y(0.0)
                    .show(ui, |p| {
                        p.line(la);
                        p.line(lb);
                    });
                ui.add_space(6.0);
            }

            ui.separator();
            ui.label(
                egui::RichText::new("Funzioni che cambiano di più (Δ self sample, B − A)").strong(),
            );
            if d.movers.is_empty() {
                ui.weak("Nessuna differenza nei self-sample (flame graph assenti o identici).");
            }
            for m in &d.movers {
                let delta = m.delta();
                let (col, sign) = if delta > 0 { (UP, "+") } else { (DOWN, "") };
                ui.horizontal(|ui| {
                    ui.colored_label(col, format!("{sign}{delta}"));
                    ui.label(format!("{}   ({} → {})", m.name, m.self_a, m.self_b));
                });
            }
        });
}

fn idx_points(v: &[f32]) -> PlotPoints {
    v.iter()
        .enumerate()
        .map(|(i, &y)| [i as f64, y as f64])
        .collect()
}
