//! Tema visivo di Argus: la palette scura di `docs/05-ui-design.md` applicata
//! sopra egui. Colori saturi al ~90% (mai 100%) per riposare gli occhi.

use eframe::egui::{self, Color32, Stroke};

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

const BACKGROUND: Color32 = rgb(0x0E, 0x0E, 0x12);
const SURFACE: Color32 = rgb(0x16, 0x16, 0x1D);
const SURFACE_2: Color32 = rgb(0x1E, 0x1E, 0x28);
const BORDER: Color32 = rgb(0x2A, 0x2A, 0x38);
const TEXT_PRIMARY: Color32 = rgb(0xE8, 0xE8, 0xEE);
const TEXT_SECONDARY: Color32 = rgb(0x94, 0x94, 0xA2);
const ACCENT: Color32 = rgb(0x7B, 0x61, 0xFF);

/// Applica il tema Argus al contesto egui (chiamato all'avvio).
pub fn apply(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();

    v.panel_fill = BACKGROUND;
    v.window_fill = SURFACE;
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.extreme_bg_color = SURFACE; // sfondo card / TextEdit
    v.faint_bg_color = SURFACE_2;
    v.hyperlink_color = ACCENT;

    // Selezione/focus = accento.
    v.selection.bg_fill = Color32::from_rgba_unmultiplied(0x7B, 0x61, 0xFF, 96);
    v.selection.stroke = Stroke::new(1.0, ACCENT);

    // Widget non interattivi (testo, separatori).
    v.widgets.noninteractive.bg_fill = SURFACE;
    v.widgets.noninteractive.weak_bg_fill = SURFACE;
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);

    // Widget inattivi (bottoni a riposo).
    v.widgets.inactive.bg_fill = SURFACE_2;
    v.widgets.inactive.weak_bg_fill = SURFACE_2;
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT_SECONDARY);

    // Hover.
    v.widgets.hovered.bg_fill = rgb(0x2A, 0x2A, 0x3A);
    v.widgets.hovered.weak_bg_fill = rgb(0x2A, 0x2A, 0x3A);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);

    // Attivo / premuto / selezionato.
    v.widgets.active.bg_fill = Color32::from_rgba_unmultiplied(0x7B, 0x61, 0xFF, 140);
    v.widgets.active.weak_bg_fill = Color32::from_rgba_unmultiplied(0x7B, 0x61, 0xFF, 140);
    v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
    v.widgets.active.fg_stroke = Stroke::new(1.0, TEXT_PRIMARY);

    ctx.set_visuals(v);

    // Spacing su griglia 4px (docs/05-ui-design).
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(8.0, 4.0);
    ctx.set_style(style);
}
