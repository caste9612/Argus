//! Colore dei frame del flame graph: una tinta calda stabile derivata dal nome.
//!
//! Condiviso tra il rendering UI (egui) e l'export SVG, così i colori
//! combaciano. Funzione pura, niente dipendenze grafiche.

/// Tinta calda (arancio→giallo) stabile per il nome di un frame. `value` è la
/// componente V di HSV (luminosità): si usa per attenuare/evidenziare.
pub fn frame_rgb(name: &str, value: f32) -> (u8, u8, u8) {
    // FNV-1a: hash stabile e ben distribuito sul nome.
    let mut h: u32 = 2_166_136_261;
    for b in name.bytes() {
        h = (h ^ b as u32).wrapping_mul(16_777_619);
    }
    let hue = 18.0 + (h % 38) as f32; // 18..56 gradi
    let sat = 0.55 + ((h >> 9) & 0xff) as f32 / 255.0 * 0.25; // 0.55..0.80
    hsv_to_rgb(hue, sat, value)
}

/// Conversione HSV→RGB. `h` in gradi (0..360), `s`/`v` in `[0,1]`.
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let c = v * s;
    let hp = (h / 60.0).rem_euclid(6.0);
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    let to = |f: f32| ((f + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    (to(r), to(g), to(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_color_is_stable_and_distinct() {
        // Stabile: stesso nome → stesso colore.
        assert_eq!(frame_rgb("main", 0.8), frame_rgb("main", 0.8));
        // Nomi diversi → (quasi sempre) colori diversi.
        assert_ne!(frame_rgb("main", 0.8), frame_rgb("compute", 0.8));
    }

    #[test]
    fn hsv_primaries() {
        assert_eq!(hsv_to_rgb(0.0, 1.0, 1.0), (255, 0, 0)); // rosso
        assert_eq!(hsv_to_rgb(120.0, 1.0, 1.0), (0, 255, 0)); // verde
        assert_eq!(hsv_to_rgb(240.0, 1.0, 1.0), (0, 0, 255)); // blu
        assert_eq!(hsv_to_rgb(0.0, 0.0, 0.0), (0, 0, 0)); // nero
    }
}
