//! Scheda "Guida": come usare Argus e, soprattutto, come *leggere* i dati.
//!
//! È la versione in-app dell'angolo didattico del progetto (la guida estesa è in
//! `docs/10-uso.md`). Testo statico, nessuno stato.

use eframe::egui::{self, RichText};

pub fn render(ui: &mut egui::Ui) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(4.0);
            ui.heading("Guida ad Argus");
            ui.label(
                "Argus ti fa vedere — e capire — come un programma usa CPU, memoria, \
                 I/O, thread e lock. Qui: come si usa e come interpretare i dati.",
            );
            ui.add_space(10.0);

            section(
                ui,
                "Come si usa",
                &[
                    "Avvia Argus come amministratore per il profiling completo \
                     (flame graph, timeline, disco, memoria — usano ETW, che richiede privilegi).",
                    "Scegli un processo nella lista a sinistra → Collega (o doppio click).",
                    "Esplora le schede: Metriche, Flame graph, Timeline, Diff.",
                    "Salva la sessione (.argus), riaprila in replay, confrontala o esportala (CSV/SVG/JSON).",
                ],
            );

            section(
                ui,
                "Le schede",
                &[
                    "Metriche — CPU, RAM (working set + private), I/O, thread, handle in tempo \
                     reale; ogni card ha una sparkline del trend e un tooltip che la spiega.",
                    "Flame graph — dove il processo spende tempo CPU: barra larga = tanto tempo. \
                     Click per zoomare, ricerca regex per evidenziare.",
                    "Timeline — stato dei thread nel tempo (Running / Ready / Waiting) e le attese \
                     per causa (Lock / I/O / Idle).",
                    "Diff — confronta due sessioni salvate (prima/dopo un'ottimizzazione).",
                ],
            );

            ui.add_space(2.0);
            ui.label(RichText::new("Come interpretare i dati").strong().size(17.0));
            ui.add_space(6.0);

            diag(
                ui,
                "È lento — dove va il tempo?",
                "CPU alta + poche attese → compute-bound: nel Flame graph la barra più larga è la \
                 funzione da ottimizzare. CPU bassa ma lento → sta aspettando: nella Timeline tante \
                 attese su Lock = contesa, su I/O = disco/rete.",
            );
            diag(
                ui,
                "Memory leak",
                "Private bytes che crescono in modo monotono (non scendono mai) = forte sospetto di \
                 leak. Sano = oscilla attorno a un valore.",
            );
            diag(
                ui,
                "Paging",
                "Molti hard page fault = il working set non sta in RAM, il sistema pagina da disco \
                 → rallentamenti a scatti.",
            );
            diag(
                ui,
                "Handle leak",
                "Handle che salgono senza mai scendere = file/socket/oggetti kernel non chiusi.",
            );
            diag(
                ui,
                "Disco sotto pressione",
                "Latenza p99 molto più alta della mediana = code di I/O occasionali.",
            );

            ui.add_space(8.0);
            ui.label(
                RichText::new(
                    "Regola d'oro: prima misura, poi ottimizza. Guida estesa: docs/10-uso.md.",
                )
                .italics()
                .weak(),
            );
            ui.add_space(8.0);
        });
}

fn section(ui: &mut egui::Ui, title: &str, lines: &[&str]) {
    ui.label(RichText::new(title).strong().size(17.0));
    ui.add_space(4.0);
    for l in lines {
        ui.label(format!("•  {l}"));
    }
    ui.add_space(12.0);
}

fn diag(ui: &mut egui::Ui, question: &str, answer: &str) {
    ui.label(RichText::new(question).strong());
    ui.label(answer);
    ui.add_space(8.0);
}
