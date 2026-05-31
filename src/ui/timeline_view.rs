//! Tab "Timeline": Gantt degli stati Running dei thread (Fase 3), dai context
//! switch ETW. Una riga per thread (i più attivi in cima), barre = intervalli in
//! esecuzione mappati sullo span temporale catturato. Disegnata col Painter di
//! egui (come il flame, D17). Si popola solo con cattura ETW attiva (admin).

use crate::aggregation::timeline::{wait_reason_name, ThreadState, ThreadTimeline};
use crate::aggregation::{FlameStatus, Snapshot, Status};
use eframe::egui;
use parking_lot::Mutex;

const ROW_H: f32 = 18.0;
const LABEL_W: f32 = 150.0;
const MAX_ROWS: usize = 48;

const RUN: egui::Color32 = egui::Color32::from_rgb(124, 217, 146); // verde: Running
const READY: egui::Color32 = egui::Color32::from_rgb(240, 198, 116); // ambra: Ready (attende CPU)
const WAIT: egui::Color32 = egui::Color32::from_rgb(124, 160, 220); // blu: Waiting
const LOCK: egui::Color32 = egui::Color32::from_rgb(255, 107, 107); // rosso: contesa lock
const AMBER: egui::Color32 = egui::Color32::from_rgb(240, 198, 116);
const GREEN: egui::Color32 = egui::Color32::from_rgb(124, 217, 146);
const GREY: egui::Color32 = egui::Color32::from_rgb(148, 148, 162);

fn state_color(s: ThreadState) -> egui::Color32 {
    match s {
        ThreadState::Running => RUN,
        ThreadState::Ready => READY,
        ThreadState::Waiting => WAIT,
        ThreadState::Other => egui::Color32::from_gray(70),
    }
}

/// Testo del tooltip per un segmento (stato, causa se in attesa, durata).
fn segment_tooltip(seg: &crate::aggregation::timeline::Segment) -> String {
    let dur = seg.end.saturating_sub(seg.start);
    match seg.state {
        ThreadState::Running => format!("Running · {dur} tick"),
        ThreadState::Ready => format!("Ready (attende CPU) · {dur} tick"),
        ThreadState::Waiting => {
            format!(
                "Waiting · {} · {dur} tick",
                wait_reason_name(seg.wait_reason)
            )
        }
        ThreadState::Other => format!("Altro · {dur} tick"),
    }
}

pub fn render(ui: &mut egui::Ui, snap: &Snapshot, timeline: &Mutex<ThreadTimeline>) {
    ui.horizontal(|ui| {
        ui.heading("Timeline thread");
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
    });
    ui.separator();

    if let FlameStatus::Unavailable(reason) = &snap.flame_status {
        banner(ui, reason);
        return;
    }
    if snap.status == Status::NotAttached {
        center(
            ui,
            "Collegati a un processo per vedere quando i suoi thread girano.",
        );
        return;
    }

    // Lock breve per layout + disegno (vedi D16).
    let t = timeline.lock();
    if t.is_empty() {
        center(
            ui,
            "In attesa di context switch… (la timeline usa ETW, come il flame)",
        );
        return;
    }
    let (t0, t1) = t.span();
    let span = (t1.saturating_sub(t0)).max(1) as f64;
    let threads = t.threads_by_busy();

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "{} thread · finestra {} tick QPC ·",
                        threads.len(),
                        t1.saturating_sub(t0)
                    ))
                    .small()
                    .weak(),
                );
                ui.colored_label(RUN, "Running");
                ui.colored_label(READY, "Ready");
                ui.colored_label(WAIT, "Waiting");
            });
            // Riepilogo attese per causa: evidenzia la contesa (Lock alto).
            let wb = t.wait_breakdown();
            let tot = wb.total();
            if tot > 0 {
                let pct = |x: u64| x as f64 / tot as f64 * 100.0;
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Attese per causa:").small().weak());
                    ui.colored_label(LOCK, format!("Lock {:.0}%", pct(wb.lock)))
                        .on_hover_text(
                            "Tempo in attesa di sincronizzazione (mutex, lock, eventi): \
                             se alto, i thread si contendono risorse.",
                        );
                    ui.colored_label(WAIT, format!("I/O {:.0}%", pct(wb.io)));
                    ui.colored_label(GREY, format!("Idle {:.0}%", pct(wb.user_idle)))
                        .on_hover_text("Attesa volontaria o thread-pool a riposo (sana).");
                    if wb.preempted > 0 {
                        ui.colored_label(READY, format!("Preempt {:.0}%", pct(wb.preempted)));
                    }
                });
            }
            let width = ui.available_width().max(80.0);
            let track_w = (width - LABEL_W).max(20.0);
            let shown = threads.len().min(MAX_ROWS);
            let height = shown as f32 * ROW_H;
            let (resp, painter) =
                ui.allocate_painter(egui::vec2(width, height), egui::Sense::hover());
            let area = resp.rect;
            let track_left = area.left() + LABEL_W;
            let hover = resp.hover_pos();
            let mut hover_text: Option<String> = None;

            for (i, (tid, busy)) in threads.iter().take(MAX_ROWS).enumerate() {
                let y = area.top() + i as f32 * ROW_H;
                let busy_pct = *busy as f64 / span * 100.0;
                // Etichetta thread.
                painter.text(
                    egui::pos2(area.left() + 2.0, y + ROW_H / 2.0),
                    egui::Align2::LEFT_CENTER,
                    format!("TID {tid}  ·  {busy_pct:.0}%"),
                    egui::FontId::proportional(10.5),
                    GREY,
                );
                // Sfondo della traccia.
                let track = egui::Rect::from_min_max(
                    egui::pos2(track_left, y + 2.0),
                    egui::pos2(area.right(), y + ROW_H - 2.0),
                );
                painter.rect_filled(track, 1.0, egui::Color32::from_gray(32));
                // Segmenti di stato (Running/Ready/Waiting) colorati.
                for seg in t.segments_of(*tid) {
                    let x0 = track_left
                        + ((seg.start.saturating_sub(t0)) as f64 / span * track_w as f64) as f32;
                    let x1 = track_left
                        + ((seg.end.saturating_sub(t0)) as f64 / span * track_w as f64) as f32;
                    let r = egui::Rect::from_min_max(
                        egui::pos2(x0, y + 2.0),
                        egui::pos2(x1.max(x0 + 1.0), y + ROW_H - 2.0),
                    );
                    painter.rect_filled(r, 1.0, state_color(seg.state));
                    // Tooltip: il segmento sotto il puntatore (l'ultimo vince).
                    if let Some(p) = hover {
                        if r.contains(p) {
                            hover_text = Some(segment_tooltip(seg));
                        }
                    }
                }
            }
            if let Some(txt) = hover_text {
                resp.on_hover_text(txt);
            }
            if threads.len() > MAX_ROWS {
                ui.weak(format!(
                    "… e altri {} thread non mostrati",
                    threads.len() - MAX_ROWS
                ));
            }
        });
}

fn banner(ui: &mut egui::Ui, reason: &str) {
    ui.add_space(40.0);
    ui.vertical_centered(|ui| {
        ui.colored_label(
            AMBER,
            egui::RichText::new("⚠ Timeline non disponibile").heading(),
        );
        ui.add_space(6.0);
        ui.label(reason);
        ui.add_space(4.0);
        ui.weak("La timeline usa i context switch ETW (kernel), che richiedono privilegi di amministratore.");
    });
}

fn center(ui: &mut egui::Ui, msg: &str) {
    ui.add_space(60.0);
    ui.vertical_centered(|ui| {
        ui.label(msg);
    });
}
