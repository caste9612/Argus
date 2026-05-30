//! Export di una sessione in formati interoperabili (Fase 4):
//!
//! - **CSV**: le storie time-series, una colonna per metrica.
//! - **folded stacks**: il formato di flamegraph.pl / speedscope (`a;b;c N`),
//!   apribile in quei tool per condividere il flame graph.
//! - **SVG**: un flame graph statico autonomo (rettangoli + tooltip `<title>`),
//!   apribile in un browser.
//!
//! Tutte funzioni **pure** (stringa in uscita): interamente testabili, niente I/O.

use crate::aggregation::flame::{FlameGraph, NodeId, ROOT};
use crate::persist::Capture;
use crate::util::color::frame_rgb;

/// Estensione file e nome leggibile per ciascun formato di export.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportKind {
    Csv,
    Folded,
    Svg,
}

impl ExportKind {
    pub fn extension(self) -> &'static str {
        match self {
            ExportKind::Csv => "csv",
            ExportKind::Folded => "folded.txt",
            ExportKind::Svg => "svg",
        }
    }
}

/// Storie time-series in CSV: una riga per indice di campione, una colonna per
/// metrica. Le storie più corte lasciano celle vuote.
pub fn to_csv(c: &Capture) -> String {
    let cols: [(&str, &Vec<f32>); 7] = [
        ("cpu_pct", &c.cpu_hist),
        ("working_set_mb", &c.ws_hist),
        ("private_mb", &c.priv_hist),
        ("io_read_mbps", &c.io_r_hist),
        ("io_write_mbps", &c.io_w_hist),
        ("threads", &c.thread_hist),
        ("handles", &c.handle_hist),
    ];
    let rows = cols.iter().map(|(_, v)| v.len()).max().unwrap_or(0);

    let mut s = String::from("sample");
    for (head, _) in &cols {
        s.push(',');
        s.push_str(head);
    }
    s.push('\n');

    for i in 0..rows {
        s.push_str(&i.to_string());
        for (_, v) in &cols {
            s.push(',');
            if let Some(x) = v.get(i) {
                s.push_str(&format!("{x}"));
            }
        }
        s.push('\n');
    }
    s
}

/// Flame graph nel formato "folded stacks": una riga per percorso radice→foglia
/// con `own > 0`, nomi separati da `;`, seguiti dal conteggio dei sample.
pub fn to_folded(g: &FlameGraph) -> String {
    let mut out = String::new();
    for id in 0..g.node_count() as NodeId {
        let own = g.own_of(id);
        if own == 0 {
            continue;
        }
        // Risali fino alla radice (esclusa) raccogliendo i nomi.
        let mut names: Vec<&str> = Vec::new();
        let mut cur = id;
        while cur != ROOT {
            names.push(g.name_of(cur));
            cur = g.parent_of(cur);
        }
        if names.is_empty() {
            continue; // own sulla radice stessa: niente da emettere
        }
        names.reverse();
        out.push_str(&names.join(";"));
        out.push(' ');
        out.push_str(&own.to_string());
        out.push('\n');
    }
    out
}

/// Flame graph come SVG statico autonomo (apribile in un browser). Ogni nodo è
/// un rettangolo colorato con un `<title>` per il tooltip.
pub fn to_svg(g: &FlameGraph) -> String {
    const W: f64 = 1200.0;
    const ROW: f64 = 16.0;

    let rects = g.layout(ROOT);
    let max_depth = rects.iter().map(|r| r.depth).max().unwrap_or(0);
    let height = (max_depth as f64 + 1.0) * ROW + 2.0;
    let total = g.total_samples().max(1);

    let mut s = String::new();
    s.push_str(&format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{W:.0}" height="{height:.0}" font-family="Consolas,monospace" font-size="11">"#
    ));
    s.push_str(&format!(
        r##"<rect width="{W:.0}" height="{height:.0}" fill="#1e1e24"/>"##
    ));

    for r in &rects {
        let x = r.x0 * W;
        let w = (r.x1 - r.x0) * W;
        if w < 0.5 {
            continue;
        }
        let y = r.depth as f64 * ROW;
        let name = g.name_of(r.node);
        let (cr, cg, cb) = frame_rgb(name, 0.85);
        let node_total = g.total_of(r.node);
        let pct = node_total as f64 / total as f64 * 100.0;
        let title = xml_escape(&format!("{name}  ({node_total} sample, {pct:.1}%)"));

        s.push_str(&format!(
            r##"<g><title>{title}</title><rect x="{x:.2}" y="{y:.2}" width="{w:.2}" height="{:.2}" fill="#{cr:02x}{cg:02x}{cb:02x}" stroke="#1e1e24" stroke-width="0.5"/>"##,
            ROW - 1.0
        ));
        if w > 28.0 {
            let max_chars = (w / 6.5) as usize;
            let label = xml_escape(&truncate_to(name, max_chars));
            s.push_str(&format!(
                r##"<text x="{:.2}" y="{:.2}" fill="#141414">{label}</text>"##,
                x + 3.0,
                y + ROW - 4.0
            ));
        }
        s.push_str("</g>");
    }
    s.push_str("</svg>\n");
    s
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn truncate_to(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        s.chars().take(max_chars.max(1)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_flame() -> FlameGraph {
        let mut g = FlameGraph::new();
        g.add_stack(&["main", "a", "b"]);
        g.add_stack(&["main", "a", "b"]);
        g.add_stack(&["main", "a"]);
        g
    }

    fn sample_capture() -> Capture {
        Capture {
            argus_version: "0.1.0".into(),
            process_name: "x".into(),
            pid: 1,
            num_cpus: 8,
            cpu_hist: vec![1.0, 2.0],
            ws_hist: vec![10.0],
            priv_hist: vec![],
            io_r_hist: vec![],
            io_w_hist: vec![],
            thread_hist: vec![],
            handle_hist: vec![],
            total_io_read_mb: 0.0,
            total_io_write_mb: 0.0,
            flame: sample_flame(),
        }
    }

    #[test]
    fn csv_has_header_and_rows() {
        let csv = to_csv(&sample_capture());
        let mut lines = csv.lines();
        assert_eq!(
            lines.next().unwrap(),
            "sample,cpu_pct,working_set_mb,private_mb,io_read_mbps,io_write_mbps,threads,handles"
        );
        // 2 righe (max len delle storie = 2).
        assert_eq!(csv.lines().count(), 3); // header + 2
                                            // Prima riga: indice 0, cpu 1, ws 10, resto vuoto.
        let row0 = csv.lines().nth(1).unwrap();
        assert!(row0.starts_with("0,1,10,"));
    }

    #[test]
    fn folded_emits_paths_with_counts() {
        let folded = to_folded(&sample_flame());
        // "main;a;b 2" (foglia b con own 2) e "main;a 1" (a con own 1).
        assert!(
            folded.contains("main;a;b 2"),
            "atteso 'main;a;b 2' in:\n{folded}"
        );
        assert!(
            folded.contains("main;a 1"),
            "atteso 'main;a 1' in:\n{folded}"
        );
    }

    #[test]
    fn svg_is_well_formed_ish() {
        let svg = to_svg(&sample_flame());
        assert!(svg.starts_with("<svg"));
        assert!(svg.trim_end().ends_with("</svg>"));
        assert!(svg.contains("<rect"));
        assert!(svg.contains("<title>"));
    }

    #[test]
    fn xml_escape_handles_specials() {
        assert_eq!(xml_escape("a<b>&\"'"), "a&lt;b&gt;&amp;&quot;&apos;");
        // I nomi C++ con template (es. vector<int>) restano validi nell'SVG.
        let svg = {
            let mut g = FlameGraph::new();
            g.add_stack(&["std::vector<int>::push_back"]);
            to_svg(&g)
        };
        assert!(svg.contains("&lt;int&gt;"));
    }
}
