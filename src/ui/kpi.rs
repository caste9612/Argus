//! Componenti riusabili della dashboard: KPI card (con mini-sparkline), card
//! "hero", grafici time-series (egui_plot) con riempimento ad area.

use crate::aggregation::{HISTORY_LEN, SAMPLE_HZ};
use eframe::egui::{self, RichText};
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

/// Mini-grafico ad area dentro una card: il trend recente a colpo d'occhio.
/// Disegnato a mano (painter) per stare compatto, senza assi né margini.
fn sparkline(ui: &mut egui::Ui, data: &VecDeque<f32>, color: egui::Color32, size: egui::Vec2) {
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    if data.len() < 2 {
        return;
    }
    let painter = ui.painter_at(rect);
    let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
    for &v in data {
        lo = lo.min(v);
        hi = hi.max(v);
    }
    if !lo.is_finite() {
        return;
    }
    let span = (hi - lo).max(1e-6);
    let n = data.len();
    // 4% di margine sopra/sotto perché la linea non tocchi i bordi.
    let pad = rect.height() * 0.06;
    let pos = |i: usize, v: f32| {
        let x = rect.left() + rect.width() * (i as f32 / (n - 1) as f32);
        let y = rect.bottom() - pad - (rect.height() - 2.0 * pad) * ((v - lo) / span);
        egui::pos2(x, y)
    };
    let pts: Vec<egui::Pos2> = data.iter().enumerate().map(|(i, &v)| pos(i, v)).collect();
    let fill = color.gamma_multiply(0.16);
    // Area: un trapezio (convesso) per segmento → tessellazione corretta.
    for i in 0..pts.len() - 1 {
        let quad = vec![
            pts[i],
            pts[i + 1],
            egui::pos2(pts[i + 1].x, rect.bottom()),
            egui::pos2(pts[i].x, rect.bottom()),
        ];
        painter.add(egui::Shape::convex_polygon(quad, fill, egui::Stroke::NONE));
    }
    for i in 0..pts.len() - 1 {
        painter.line_segment([pts[i], pts[i + 1]], egui::Stroke::new(1.5, color));
    }
}

/// Card "hero": metrica protagonista (CPU) — valore grande + sparkline ampia.
pub fn hero(
    ui: &mut egui::Ui,
    label: &str,
    value: &str,
    color: egui::Color32,
    hint: &str,
    hist: &VecDeque<f32>,
) {
    egui::Frame::group(ui.style())
        .fill(ui.style().visuals.extreme_bg_color)
        .inner_margin(egui::Margin::symmetric(14.0, 10.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(label).weak());
                    ui.label(RichText::new(value).monospace().size(38.0).color(color));
                });
                ui.add_space(18.0);
                let w = (ui.available_width() - 8.0).max(80.0);
                ui.vertical(|ui| {
                    ui.add_space(10.0);
                    sparkline(ui, hist, color, egui::vec2(w, 58.0));
                });
            });
        })
        .response
        .on_hover_text(hint);
}

/// Card metrica compatta: label piccola, valore monospace colorato, sparkline.
pub fn metric_card(
    ui: &mut egui::Ui,
    label: &str,
    value: &str,
    color: egui::Color32,
    hint: &str,
    hist: &VecDeque<f32>,
) {
    egui::Frame::group(ui.style())
        .fill(ui.style().visuals.extreme_bg_color)
        .inner_margin(egui::Margin::symmetric(10.0, 7.0))
        .show(ui, |ui| {
            // Riempie la colonna assegnata (la griglia in dashboard.rs decide
            // quante colonne): così la card è responsive come hero e grafici.
            let w = ui.available_width();
            ui.set_width(w);
            ui.vertical(|ui| {
                ui.label(RichText::new(label).small().weak());
                ui.label(RichText::new(value).monospace().size(20.0).color(color));
                sparkline(ui, hist, color, egui::vec2(w, 26.0));
            });
        })
        .response
        .on_hover_text(hint);
}

/// Card semplice (label + valore), senza sparkline. Usata nelle sezioni
/// disco/memoria dentro `ui.columns(...)`: riempie la colonna assegnata, così
/// le card della sezione restano uniformi e allineate (niente larghezze ragged).
pub fn card(ui: &mut egui::Ui, label: &str, value: &str, color: egui::Color32, hint: &str) {
    egui::Frame::group(ui.style())
        .fill(ui.style().visuals.extreme_bg_color)
        .inner_margin(egui::Margin::symmetric(10.0, 6.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical(|ui| {
                ui.label(RichText::new(label).small().weak());
                ui.label(RichText::new(value).monospace().size(20.0).color(color));
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
        .label_formatter(|_name, p| {
            let ago = (window_secs() - p.x).max(0.0);
            format!("{ago:.0}s fa · {:.2}", p.y)
        })
}

/// Grafico a due serie sovrapposte (es. read/write), entrambe ad area.
pub fn chart_dual(
    ui: &mut egui::Ui,
    title: &str,
    (label_a, data_a, color_a): (&str, &VecDeque<f32>, egui::Color32),
    (label_b, data_b, color_b): (&str, &VecDeque<f32>, egui::Color32),
    height: f32,
) {
    let (ca, cb) = (
        data_a.back().copied().unwrap_or(0.0),
        data_b.back().copied().unwrap_or(0.0),
    );
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).strong());
        ui.colored_label(color_a, format!("● {label_a} {ca:.1}"));
        ui.colored_label(color_b, format!("● {label_b} {cb:.1}"));
    });
    let la = Line::new(points(data_a))
        .color(color_a)
        .width(1.6)
        .fill(0.0);
    let lb = Line::new(points(data_b))
        .color(color_b)
        .width(1.6)
        .fill(0.0);
    base_plot(title, height).show(ui, |pui| {
        pui.line(la);
        pui.line(lb);
    });
}
