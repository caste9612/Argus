//! Tab "Flame graph": rendering interattivo del flame graph col `Painter` di
//! egui (un rettangolo per nodo). Per il numero di nodi in gioco (migliaia) è
//! performante e affidabile; una pipeline wgpu custom resta un'ottimizzazione
//! futura per grafi enormi (vedi D17 in docs/09-decisions.md).
//!
//! Interazioni: click su un frame per zoomare (il focus si espande a piena
//! larghezza, con gli antenati cliccabili per tornare su); casella di ricerca
//! per evidenziare le funzioni che contengono il testo; hover per il dettaglio.

use crate::aggregation::flame::{FlameGraph, NodeId, ROOT};
use crate::aggregation::{FlameStatus, Snapshot, Status};
use eframe::egui;
use parking_lot::Mutex;

/// Altezza di una riga (un livello di stack) in pixel.
const ROW_H: f32 = 18.0;

const AMBER: egui::Color32 = egui::Color32::from_rgb(240, 198, 116);
const GREEN: egui::Color32 = egui::Color32::from_rgb(124, 217, 146);
const GREY: egui::Color32 = egui::Color32::from_rgb(148, 148, 162);

pub fn render(
    ui: &mut egui::Ui,
    snap: &Snapshot,
    flame: &Mutex<FlameGraph>,
    focus: &mut NodeId,
    search: &mut String,
) {
    header(ui, snap, focus, search);
    ui.separator();

    if let FlameStatus::Unavailable(reason) = &snap.flame_status {
        banner(ui, reason);
        return;
    }
    if snap.status == Status::NotAttached {
        info_center(
            ui,
            "Collegati a un processo per vedere dove spende tempo CPU.",
        );
        return;
    }

    // Manteniamo il lock per layout + disegno: sezioni brevi (vedi D16). Le
    // risoluzioni costose avvengono nell'aggregatore, fuori dal lock.
    let g = flame.lock();
    let total = g.total_samples();
    if total == 0 {
        let msg = match snap.flame_status {
            FlameStatus::Active => "Cattura avviata: in attesa dei primi stack sample…",
            _ => "Nessuno stack campionato.",
        };
        info_center(ui, msg);
        return;
    }
    // Focus non più valido (albero azzerato): torna alla radice.
    if *focus != ROOT && g.total_of(*focus) == 0 {
        *focus = ROOT;
    }
    if let Some(clicked) = draw_flame(ui, &g, *focus, search, total) {
        *focus = clicked;
    }
}

fn header(ui: &mut egui::Ui, snap: &Snapshot, focus: &mut NodeId, search: &mut String) {
    ui.horizontal(|ui| {
        ui.heading("Flame graph");
        ui.separator();
        match &snap.flame_status {
            FlameStatus::Active => {
                ui.colored_label(GREEN, "● cattura ETW attiva");
            }
            FlameStatus::Unavailable(_) => {
                ui.colored_label(AMBER, "● ETW non disponibile");
            }
            FlameStatus::Off => {
                ui.colored_label(GREY, "○ cattura ferma");
            }
        }
        ui.separator();
        ui.add(
            egui::TextEdit::singleline(search)
                .hint_text("evidenzia funzione…")
                .desired_width(200.0),
        );
        if ui
            .button("⟲ zoom out")
            .on_hover_text("Torna alla radice")
            .clicked()
        {
            *focus = ROOT;
        }
    });
}

/// Disegna il flame graph. Ritorna il nodo cliccato (nuovo focus), se c'è.
fn draw_flame(
    ui: &mut egui::Ui,
    g: &FlameGraph,
    focus: NodeId,
    search: &str,
    total: u64,
) -> Option<NodeId> {
    let rects = g.layout(focus);
    let max_depth = rects.iter().map(|r| r.depth).max().unwrap_or(0);
    let query = search.trim().to_lowercase();

    let mut clicked = None;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let width = ui.available_width().max(50.0);
            let height = (max_depth as f32 + 1.0) * ROW_H;
            let (resp, painter) =
                ui.allocate_painter(egui::vec2(width, height), egui::Sense::click());
            let area = resp.rect;
            let hover_pos = resp.hover_pos();
            let mut hovered: Option<NodeId> = None;

            for r in &rects {
                let x0 = area.left() + (r.x0 as f32) * area.width();
                let x1 = area.left() + (r.x1 as f32) * area.width();
                if x1 - x0 < 1.0 {
                    continue; // sotto il pixel: invisibile, saltato
                }
                let y = area.top() + (r.depth as f32) * ROW_H;
                let cell = egui::Rect::from_min_max(
                    egui::pos2(x0, y),
                    egui::pos2(x1 - 1.0, y + ROW_H - 1.0),
                );

                let name = g.name_of(r.node);
                let matched = !query.is_empty() && name.to_lowercase().contains(&query);
                let dim = !query.is_empty() && !matched;
                let fill = frame_color(name, dim, matched);
                painter.rect_filled(cell, egui::Rounding::same(2.0), fill);

                if cell.width() > 26.0 {
                    let text_color = egui::Color32::from_gray(20);
                    let clip = painter.with_clip_rect(cell.shrink2(egui::vec2(4.0, 0.0)));
                    clip.text(
                        egui::pos2(x0 + 4.0, y + ROW_H / 2.0),
                        egui::Align2::LEFT_CENTER,
                        name,
                        egui::FontId::proportional(11.0),
                        text_color,
                    );
                }

                if hover_pos.is_some_and(|p| cell.contains(p)) {
                    hovered = Some(r.node);
                    painter.rect_stroke(
                        cell,
                        egui::Rounding::same(2.0),
                        egui::Stroke::new(1.5, egui::Color32::WHITE),
                    );
                }
            }

            if let Some(node) = hovered {
                tooltip(ui, g, node, total);
                if resp.clicked() {
                    clicked = Some(node);
                }
            }
        });
    clicked
}

/// Tooltip con nome completo, sample e percentuali (self/totale).
fn tooltip(ui: &egui::Ui, g: &FlameGraph, node: NodeId, total: u64) {
    let name = g.name_of(node).to_string();
    let node_total = g.total_of(node);
    let node_own = g.own_of(node);
    let pct = |n: u64| {
        if total > 0 {
            n as f64 / total as f64 * 100.0
        } else {
            0.0
        }
    };
    egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), egui::Id::new("flame_tip"), |ui| {
        ui.strong(name);
        ui.label(format!(
            "{} sample · {:.1}% del totale",
            node_total,
            pct(node_total)
        ));
        ui.label(format!("self: {} sample · {:.1}%", node_own, pct(node_own)));
        ui.weak("click per zoomare");
    });
}

/// Colore stabile per funzione (hash del nome → tinta calda), attenuabile per la
/// ricerca. Tinte calde (rosso-arancio-giallo) per il classico look "fiamma".
fn frame_color(name: &str, dim: bool, matched: bool) -> egui::Color32 {
    // FNV-1a per una tinta stabile e ben distribuita sul nome.
    let mut h: u32 = 2_166_136_261;
    for b in name.bytes() {
        h = (h ^ b as u32).wrapping_mul(16_777_619);
    }
    let hue = 18.0 + (h % 38) as f32; // 18..56 gradi: arancio→giallo
    let sat = 0.55 + ((h >> 9) & 0xff) as f32 / 255.0 * 0.25; // 0.55..0.80
    let val = if dim {
        0.32
    } else if matched {
        0.95
    } else {
        0.82
    };
    hsv(hue, sat, val)
}

/// Conversione HSV→Color32 (h in gradi 0..360). Evita dipendenze esterne.
fn hsv(h: f32, s: f32, v: f32) -> egui::Color32 {
    let c = v * s;
    let hp = h / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    egui::Color32::from_rgb(
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}

fn banner(ui: &mut egui::Ui, reason: &str) {
    ui.add_space(40.0);
    ui.vertical_centered(|ui| {
        ui.colored_label(
            AMBER,
            egui::RichText::new("⚠ Flame graph non disponibile").heading(),
        );
        ui.add_space(6.0);
        ui.label(reason);
        ui.add_space(4.0);
        ui.weak(
            "Il flame graph usa ETW (kernel sampling), che richiede privilegi di \
             amministratore. Le altre metriche continuano a funzionare.",
        );
    });
}

fn info_center(ui: &mut egui::Ui, msg: &str) {
    ui.add_space(60.0);
    ui.vertical_centered(|ui| {
        ui.label(msg);
    });
}
