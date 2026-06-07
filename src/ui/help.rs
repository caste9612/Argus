//! Scheda "Guida": una mini-guida al **monitoraggio delle performance**.
//!
//! Non è un elenco di bottoni: spiega i *concetti* (come funziona il campionamento
//! CPU, gli stati dei thread, i page fault, le code di I/O) e un *metodo* per
//! diagnosticare problemi reali. È il cuore didattico di Argus, in-app; la versione
//! testuale estesa vive in `docs/10-uso.md`. Testo statico, nessuno stato.

use eframe::egui::{self, RichText};

pub fn render(ui: &mut egui::Ui) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(4.0);
            ui.heading("Imparare a monitorare i software");
            ui.label(
                "Argus non serve solo a *vedere* dei numeri, ma a capire cosa succede sotto il \
                 cofano di un programma. Qui sotto i concetti che servono e un metodo per usarli \
                 a diagnosticare problemi veri. Espandi le sezioni che ti interessano.",
            );
            ui.add_space(10.0);

            topic(ui, "Il metodo: prima di tutto — calcola o aspetta?", true, &[
                "Quasi ogni problema di performance si riduce a una domanda: il programma è \
                 limitato dalla CPU (sta calcolando) o sta aspettando qualcosa (disco, rete, un \
                 lock, un timer)? La risposta decide dove guardare.",
                "Guarda la CPU del processo nelle Metriche. Se è ALTA e costante (vicina a \
                 100% × il numero di core che usa) → è compute-bound: il collo di bottiglia è il \
                 calcolo, vai al Flame graph per trovare QUALE funzione. Se è BASSA ma il \
                 programma è comunque lento → non sta calcolando, sta ASPETTANDO: vai alla \
                 Timeline per capire COSA aspetta.",
                "Perché funziona: in ogni istante un thread può solo essere in esecuzione, pronto \
                 in coda per la CPU, o in attesa di un evento. Se non usa CPU e non finisce il \
                 lavoro, per forza sta aspettando — e la Timeline ti dice di cosa.",
            ]);

            topic(ui, "CPU e flame graph — dove va il tempo", false, &[
                "User time vs kernel time. Il tempo CPU si divide in 'user' (il tuo codice) e \
                 'kernel' (le chiamate al sistema operativo: I/O, allocazioni, lock di sistema). \
                 Molto kernel time = fai tante syscall, spesso I/O o allocazioni frequenti.",
                "Come Argus vede dove va la CPU. Ogni ~1 ms il kernel (via ETW, provider PerfInfo) \
                 fotografa lo stack di chiamate dei thread in esecuzione. Migliaia di queste foto, \
                 aggregate, dicono dove il programma passava la maggior parte del tempo. È \
                 profiling a CAMPIONAMENTO: statistico, con overhead bassissimo, ma non vede \
                 funzioni più brevi dell'intervallo di campionamento.",
                "Leggere il flame graph. Ogni barra è una funzione; le barre impilate sono lo \
                 stack (in basso chi chiama, salendo i chiamati). La LARGHEZZA è proporzionale al \
                 tempo CPU (quanti campioni includevano quella funzione). Cerca la barra larga e \
                 inaspettata: è lì che si brucia tempo.",
                "Self vs Totale: 'self' è il tempo speso DENTRO la funzione stessa; 'totale' è \
                 self più tutti i suoi figli. Una funzione con self alto è dove la CPU lavora \
                 davvero — le altre stanno solo aspettando i risultati dei figli.",
                "Trabocchetto: il flame graph mostra solo tempo CPU. Il tempo passato fermi ad \
                 aspettare (I/O, lock) NON compare qui — per quello serve la Timeline.",
            ]);

            topic(ui, "Thread, scheduling e lock — cosa aspetta", false, &[
                "Gli stati di un thread. Running = sta girando su una CPU adesso. Ready = vorrebbe \
                 la CPU ma sono tutte occupate, è in coda (tanto tempo Ready = troppo lavoro per i \
                 core, o troppi thread). Waiting = aspetta un evento: la fine di un I/O, un lock, \
                 un timer, un segnale.",
                "Context switch. È quando il sistema operativo sospende un thread e ne mette un \
                 altro sulla CPU. Sono normali; ma TROPPI, insieme a tanto tempo Ready, indicano \
                 oversubscription — più thread attivi dei core, che si rubano la CPU a vicenda \
                 (e ogni switch ha un costo: cache fredda, salvataggio del contesto).",
                "Contesa sui lock (il caso più insidioso). Se il thread A tiene un lock e B lo \
                 vuole, B va in Waiting finché A non lo rilascia. Nella Timeline lo vedi come tempo \
                 Waiting con causa 'Lock'. Tanto tempo lì significa che i thread si SERIALIZZANO su \
                 quel lock: magari ne hai lanciati 8, ma di fatto ne lavora uno alla volta — il \
                 parallelismo è solo apparente. Rimedi: accorciare la sezione critica, lock più \
                 granulari, o strutture lock-free.",
            ]);

            topic(ui, "Memoria — working set, page fault, leak", false, &[
                "Le grandezze. Working set = le pagine FISICAMENTE in RAM del processo adesso. \
                 Private bytes = memoria committed non condivisa: è l'uso 'vero' del programma. \
                 Committed = memoria con RAM o pagefile dietro (usabile). Reserved = solo spazio \
                 di indirizzi prenotato, senza RAM dietro.",
                "Page fault. Succede quando il programma tocca una pagina non pronta. SOFT: la \
                 pagina è già in RAM (solo non nel working set) o è azzerata → economico. HARD: la \
                 pagina è su DISCO (pagefile o file mappato) → il SO deve leggerla, costa \
                 millisecondi. Tanti hard fault = stai PAGINANDO, il working set non sta in RAM → \
                 rallentamenti a scatti. Argus conta proprio gli hard fault.",
                "Perché Argus mostra VirtualAlloc e non ogni malloc. Tracciare ogni allocazione \
                 dell'heap richiederebbe di iniettarsi nel processo o di farlo partire con il \
                 tracing attivo. Argus non modifica mai il target (vincolo non negoziabile), quindi \
                 mostra le VirtualAlloc: le grandi riserve di memoria da cui poi l'allocatore \
                 ritaglia i piccoli oggetti. Vedi la tendenza, non ogni singolo oggetto.",
                "Come appare un leak. Private bytes / committed che salgono e NON scendono mai. Un \
                 programma sano alloca e libera, quindi oscilla attorno a un valore.",
            ]);

            topic(ui, "Disco e I/O — throughput vs latenza", false, &[
                "Throughput non è latenza. Il throughput (MB/s) dice QUANTO sposti; la latenza dice \
                 QUANTO CI METTE una singola operazione. Un disco può avere buon throughput ma \
                 latenza pessima quando è sotto coda.",
                "Percentili (p50/p99). La mediana (p50) è il caso tipico. Il p99 è la CODA: l'1% di \
                 operazioni più lente. Per la reattività percepita conta il p99, non la media: se \
                 p99 è molto più alto di p50, il disco ogni tanto si impunta (code occasionali). \
                 Argus calcola questi percentili dagli eventi di I/O del disco.",
            ]);

            topic(ui, "Handle e risorse", false, &[
                "Gli handle sono riferimenti a oggetti del kernel: file aperti, socket, mutex, \
                 thread, ecc. Ognuno va CHIUSO quando non serve più. Se il loro numero cresce in \
                 modo monotono (sale e non scende mai) c'è un handle leak: prima o poi il programma \
                 esaurisce le risorse e inizia a fallire le operazioni. Un programma sano oscilla \
                 attorno a un numero stabile.",
            ]);

            topic(ui, "Come si usa Argus (in pratica)", false, &[
                "1. Avvialo COME AMMINISTRATORE per il profiling completo (flame, timeline, disco, \
                 memoria usano ETW, che richiede privilegi).",
                "2. Scegli un processo nella lista a sinistra → Collega (o doppio click).",
                "3. Parti dalle Metriche (calcola o aspetta?), poi scendi nel Flame graph (DOVE va \
                 la CPU) o nella Timeline (COSA aspetta).",
                "4. Salva una sessione, ottimizza il codice, ricollega e usa Diff: i 'movers' \
                 mostrano cosa è cambiato — la prova che l'ottimizzazione ha funzionato.",
                "Regola d'oro: prima MISURA, poi ottimizza. Le ottimizzazioni 'a sensazione' sono \
                 quasi sempre sbagliate: il collo di bottiglia non è mai dove pensi.",
            ]);

            topic(ui, "Glossario veloce", false, &[
                "ETW — Event Tracing for Windows: il sistema di tracing del kernel da cui Argus \
                 prende flame graph, context switch, page fault e I/O (richiede admin).",
                "Sampling profiler — misura 'fotografando' lo stack a intervalli regolari, invece \
                 di strumentare ogni funzione: poco overhead, risultati statistici.",
                "Context switch — il cambio del thread in esecuzione su una CPU.",
                "Working set — le pagine di un processo residenti in RAM in questo momento.",
                "Hard page fault — accesso a una pagina che sta su disco: lento (millisecondi).",
                "Lock contention — più thread che si contendono lo stesso lock e si serializzano.",
                "p99 — il 99° percentile: il valore sotto cui sta il 99% dei casi (misura la coda).",
            ]);

            ui.add_space(6.0);
            ui.label(
                RichText::new("Guida testuale estesa: docs/10-uso.md. Regola d'oro: prima misura, poi ottimizza.")
                    .italics()
                    .weak(),
            );
            ui.add_space(8.0);
        });
}

fn topic(ui: &mut egui::Ui, title: &str, open: bool, paragraphs: &[&str]) {
    egui::CollapsingHeader::new(RichText::new(title).strong().size(16.0))
        .default_open(open)
        .show(ui, |ui| {
            ui.add_space(2.0);
            for p in paragraphs {
                ui.label(*p);
                ui.add_space(7.0);
            }
        });
    ui.add_space(3.0);
}
