//! Componenti riusabili: KPI card e grafici time-series (egui_plot).
//!
//! In Fase 2 i grafici più pesanti (flame graph) passeranno a pipeline wgpu
//! custom nel layer `viz/`; per le time-series di Fase 1 egui_plot basta.

use crate::aggregation::{HISTORY_LEN, SAMPLE_HZ};
use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints};
use std::collections::VecDeque;

/// Larghezza temporale della finestra visibile, in secondi.
fn window_secs() -> f64 {
    HISTORY_LEN as f64 / SAMPLE_HZ as f64
}

pub fn cpu_color(pct: f32) -> egui::Color32 {
    if pct < 25.0 {
        egui::Color32::from_rgb(124, 217, 146)
    } else if pct < 60.0 {
        egui::Color32::from_rgb(240, 198, 116)
    } else {
        egui::Color32::from_rgb(255, 107, 107)
    }
}

/// Card con label piccola sopra e valore grande colorato sotto.
pub fn card(ui: &mut egui::Ui, label: &str, value: &str, color: egui::Color32, hint: &str) {
    egui::Frame::group(ui.style())
        .fill(ui.style().visuals.extreme_bg_color)
        .inner_margin(egui::Margin::symmetric(10.0, 6.0))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(label).small().weak());
                ui.label(egui::RichText::new(value).heading().color(color));
            });
        })
        .response
        .on_hover_text(hint);
}

/// Converte una storia in punti (x = secondi, più recente a destra).
fn points(data: &VecDeque<f32>) -> PlotPoints {
    let n = data.len();
    let w = window_secs();
    data.iter()
        .enumerate()
        .map(|(i, &y)| {
            let x = w - (n - 1 - i) as f64 / SAMPLE_HZ as f64;
            [x, y as f64]
        })
        .collect()
}

fn base_plot(id: &str, height: f32) -> Plot<'_> {
    Plot::new(id)
        .height(height)
        .show_axes([false, true])
        .show_grid([false, true])
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .include_x(0.0)
        .include_x(window_secs())
        .include_y(0.0)
}

/// Grafico a singola serie.
pub fn chart(
    ui: &mut egui::Ui,
    title: &str,
    data: &VecDeque<f32>,
    height: f32,
    color: egui::Color32,
    y_max: Option<f64>,
) {
    ui.label(egui::RichText::new(title).strong());
    let line = Line::new(points(data)).color(color).width(1.6);
    let mut plot = base_plot(title, height);
    if let Some(m) = y_max {
        plot = plot.include_y(m);
    }
    plot.show(ui, |pui| pui.line(line));
}

/// Grafico a due serie sovrapposte (es. read/write).
pub fn chart_dual(
    ui: &mut egui::Ui,
    title: &str,
    (label_a, data_a, color_a): (&str, &VecDeque<f32>, egui::Color32),
    (label_b, data_b, color_b): (&str, &VecDeque<f32>, egui::Color32),
    height: f32,
) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).strong());
        ui.colored_label(color_a, format!("● {label_a}"));
        ui.colored_label(color_b, format!("● {label_b}"));
    });
    let la = Line::new(points(data_a)).color(color_a).width(1.6);
    let lb = Line::new(points(data_b)).color(color_b).width(1.6);
    base_plot(title, height).show(ui, |pui| {
        pui.line(la);
        pui.line(lb);
    });
}
