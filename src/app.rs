//! Wiring dell'applicazione: stato condiviso, thread sampler, ciclo eframe.

use crate::capture::sampler::{self, Command, Shared};
use crate::ui;
use eframe::egui;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;
use tracing::warn;

pub struct ArgusApp {
    shared: Arc<Shared>,
    cmd_tx: crossbeam_channel::Sender<Command>,
    join: Option<JoinHandle<()>>,
    ui: ui::State,
    cmd_buf: Vec<Command>,
    /// PID da collegare automaticamente al primo frame (opzione `--attach`).
    pending_attach: Option<u32>,
    /// Tab iniziale (opzione `--tab`); altrimenti Flame se c'è auto-attach.
    pending_tab: Option<ui::Tab>,
}

impl ArgusApp {
    /// Crea l'app e avvia il thread sampler. `initial_pid` (da `--attach <pid>`)
    /// viene collegato automaticamente al primo frame; `initial_tab` (da `--tab`)
    /// sceglie la tab iniziale (default: Flame se c'è auto-attach).
    pub fn new(initial_pid: Option<u32>, initial_tab: Option<ui::Tab>) -> Self {
        let shared = Shared::new();
        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded::<Command>();

        let shared_for_thread = shared.clone();
        let join = std::thread::Builder::new()
            .name("argus-sampler".into())
            .spawn(move || sampler::run(shared_for_thread, cmd_rx))
            .expect("impossibile avviare il thread sampler");

        Self {
            shared,
            cmd_tx,
            join: Some(join),
            ui: ui::State::default(),
            cmd_buf: Vec::new(),
            pending_attach: initial_pid,
            pending_tab: initial_tab,
        }
    }
}

impl eframe::App for ArgusApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Auto-attach + tab iniziale da riga di comando (una sola volta).
        if let Some(pid) = self.pending_attach.take() {
            let _ = self.cmd_tx.send(Command::Attach(pid));
            self.ui.tab = ui::Tab::Flame; // default per --attach
        }
        if let Some(tab) = self.pending_tab.take() {
            self.ui.tab = tab; // --tab ha la precedenza
        }

        // Snapshot immutabili: letti senza mai bloccare il sampler.
        let snap = self.shared.metrics.load_full();
        let procs = self.shared.processes.load_full();
        let captures = self.shared.captures.load_full();
        let diff = self.shared.diff.load_full();

        self.cmd_buf.clear();
        ui::render(
            ctx,
            &mut self.ui,
            &snap,
            &procs,
            &self.shared.flame,
            &self.shared.timeline,
            &self.shared.disk,
            &self.shared.mem,
            &captures,
            (*diff).as_ref(),
            &mut self.cmd_buf,
        );
        for cmd in self.cmd_buf.drain(..) {
            if self.cmd_tx.send(cmd).is_err() {
                warn!("canale comandi chiuso: il sampler non risponde");
            }
        }

        // Cadenza di repaint adattiva: non sprecare CPU/GPU quando non c'è nulla
        // da aggiornare. I dati arrivano a ≤10 Hz e ogni input forza comunque un
        // repaint immediato (egui), quindi 30 fps continui a riposo erano solo
        // consumo. Attivo → fluido; fermo → rallenta; minimizzato → quasi nulla.
        let minimized = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
        let active = matches!(
            snap.status,
            crate::aggregation::Status::Running | crate::aggregation::Status::Replay(_)
        );
        let next = if minimized {
            Duration::from_secs(1)
        } else if active {
            Duration::from_millis(50) // collegato/replay: ~20 fps (dati a 10 Hz; l'input forza comunque un repaint immediato)
        } else {
            Duration::from_millis(400) // non collegato / target uscito: risparmio
        };
        ctx.request_repaint_after(next);
    }
}

impl Drop for ArgusApp {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(Command::Shutdown);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}
