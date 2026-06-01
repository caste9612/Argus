//! Confronto fra due sessioni catturate (Fase 4): "prima/dopo".
//!
//! Produce un `DiffSummary` con le serie time-series sovrapponibili (A vs B) e
//! i "movers": le funzioni il cui tempo CPU (self sample) cambia di più tra le
//! due sessioni. Logica **pura**, interamente testabile.

use crate::aggregation::flame::{FlameGraph, NodeId};
use crate::persist::Capture;
use std::collections::HashMap;

/// Una metrica confrontata tra A e B: le due storie + sintesi (media/picco).
#[derive(Clone)]
pub struct SeriesDiff {
    pub label: String,
    pub a: Vec<f32>,
    pub b: Vec<f32>,
    pub avg_a: f32,
    pub avg_b: f32,
    pub peak_a: f32,
    pub peak_b: f32,
}

/// Differenza di self-time (in sample) di una funzione tra A e B.
#[derive(Clone)]
pub struct FuncDiff {
    pub name: String,
    pub self_a: u64,
    pub self_b: u64,
}

impl FuncDiff {
    /// Variazione assoluta dei self sample (B − A).
    pub fn delta(&self) -> i64 {
        self.self_b as i64 - self.self_a as i64
    }
}

/// Risultato del confronto fra due sessioni.
#[derive(Clone)]
pub struct DiffSummary {
    pub label_a: String,
    pub label_b: String,
    pub series: Vec<SeriesDiff>,
    /// Funzioni ordinate per variazione assoluta di self-time decrescente.
    pub movers: Vec<FuncDiff>,
}

/// Massimo numero di "movers" riportati.
const MAX_MOVERS: usize = 25;

/// Confronta due capture (A = baseline, B = nuova).
pub fn diff(a: &Capture, b: &Capture) -> DiffSummary {
    let series = vec![
        series("CPU (%)", &a.cpu_hist, &b.cpu_hist),
        series("Working set (MB)", &a.ws_hist, &b.ws_hist),
        series("Private (MB)", &a.priv_hist, &b.priv_hist),
        series("I/O lettura (MB/s)", &a.io_r_hist, &b.io_r_hist),
        series("I/O scrittura (MB/s)", &a.io_w_hist, &b.io_w_hist),
        series("Thread", &a.thread_hist, &b.thread_hist),
        series("Handle", &a.handle_hist, &b.handle_hist),
    ];

    let map_a = self_by_function(&a.flame);
    let map_b = self_by_function(&b.flame);
    let mut names: Vec<&String> = map_a.keys().chain(map_b.keys()).collect();
    names.sort_unstable();
    names.dedup();

    let mut movers: Vec<FuncDiff> = names
        .into_iter()
        .map(|name| FuncDiff {
            name: name.clone(),
            self_a: map_a.get(name).copied().unwrap_or(0),
            self_b: map_b.get(name).copied().unwrap_or(0),
        })
        .filter(|f| f.delta() != 0)
        .collect();
    movers.sort_by(|x, y| {
        y.delta()
            .abs()
            .cmp(&x.delta().abs())
            .then(x.name.cmp(&y.name))
    });
    movers.truncate(MAX_MOVERS);

    DiffSummary {
        label_a: a.process_name.clone(),
        label_b: b.process_name.clone(),
        series,
        movers,
    }
}

fn series(label: &str, a: &[f32], b: &[f32]) -> SeriesDiff {
    SeriesDiff {
        label: label.to_string(),
        a: a.to_vec(),
        b: b.to_vec(),
        avg_a: avg(a),
        avg_b: avg(b),
        peak_a: peak(a),
        peak_b: peak(b),
    }
}

fn avg(v: &[f32]) -> f32 {
    if v.is_empty() {
        0.0
    } else {
        v.iter().sum::<f32>() / v.len() as f32
    }
}

fn peak(v: &[f32]) -> f32 {
    v.iter().copied().fold(0.0_f32, f32::max)
}

/// Somma dei self-sample per nome di funzione in un flame graph.
fn self_by_function(g: &FlameGraph) -> HashMap<String, u64> {
    let mut map: HashMap<String, u64> = HashMap::new();
    for id in 0..g.node_count() as NodeId {
        let own = g.own_of(id);
        if own > 0 {
            *map.entry(g.name_of(id).to_string()).or_insert(0) += own;
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cap(name: &str, cpu: Vec<f32>, stacks: &[&[&str]]) -> Capture {
        let mut flame = FlameGraph::new();
        for s in stacks {
            flame.add_stack(s);
        }
        Capture {
            argus_version: "0.1.0".into(),
            process_name: name.into(),
            pid: 1,
            num_cpus: 8,
            cpu_hist: cpu,
            ws_hist: vec![],
            priv_hist: vec![],
            io_r_hist: vec![],
            io_w_hist: vec![],
            thread_hist: vec![],
            handle_hist: vec![],
            total_io_read_mb: 0.0,
            total_io_write_mb: 0.0,
            flame,
            wait: Default::default(),
            mem: Default::default(),
            disk: Default::default(),
        }
    }

    #[test]
    fn series_stats_and_overlay() {
        let a = cap("base", vec![10.0, 20.0, 30.0], &[]);
        let b = cap("new", vec![5.0, 5.0], &[]);
        let d = diff(&a, &b);
        let cpu = &d.series[0];
        assert_eq!(cpu.label, "CPU (%)");
        assert_eq!(cpu.a, vec![10.0, 20.0, 30.0]);
        assert_eq!(cpu.b, vec![5.0, 5.0]);
        assert_eq!(cpu.avg_a, 20.0);
        assert_eq!(cpu.peak_a, 30.0);
        assert_eq!(cpu.avg_b, 5.0);
    }

    #[test]
    fn movers_rank_functions_by_self_delta() {
        // A: hot() campionata 1 volta. B: hot() 5 volte + cold() 1.
        let a = cap("base", vec![], &[&["main", "hot"]]);
        let b = cap(
            "new",
            vec![],
            &[
                &["main", "hot"],
                &["main", "hot"],
                &["main", "hot"],
                &["main", "hot"],
                &["main", "hot"],
                &["main", "cold"],
            ],
        );
        let d = diff(&a, &b);
        assert!(!d.movers.is_empty());
        // Il mover principale è "hot" (Δ self = +4), poi "cold" (+1).
        assert_eq!(d.movers[0].name, "hot");
        assert_eq!(d.movers[0].delta(), 4);
        assert!(d.movers.iter().any(|m| m.name == "cold" && m.delta() == 1));
        // "main" ha own 0 in entrambe → non compare tra i movers.
        assert!(!d.movers.iter().any(|m| m.name == "main"));
    }
}
