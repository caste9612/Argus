# 01 – Visione

## Il problema

Capire perché un programma è lento, consuma troppa memoria, o si blocca, è ancora oggi un'arte oscura. Gli strumenti disponibili su Windows sono o:

- **Troppo low-level** (PerfView, Windows Performance Analyzer): potentissimi ma richiedono giorni di training per interpretare gli output, generano file enormi, UI proibitiva.
- **Troppo high-level** (Task Manager, Process Explorer): mostrano metriche aggregate ma non aiutano a capire *dove* nel codice sta il problema.
- **Specifici per un linguaggio** (Visual Studio Profiler, dotTrace): legati a un runtime e a un IDE.

Manca uno strumento **per sviluppatori che pensa come uno sviluppatore**: dati profondi, presentati in modo comprensibile, sempre disponibili come singolo eseguibile.

## L'obiettivo di Argus

Permettere a uno sviluppatore di:

1. Lanciare un singolo `.exe`
2. Selezionare un processo in esecuzione
3. Vedere immediatamente — in una dashboard fluida e GPU-renderizzata — **dove** quel processo spende tempo, memoria, I/O, e **perché**
4. Identificare il problema senza dover esportare file, aprire altri tool, o conoscere ETW

## Per chi è

Sviluppatori di:

- **Software nativo** (Rust, C++, Go, Zig) che vogliono profiling senza ricompilare con flag speciali
- **Software .NET** che cercano un complemento ai profiler integrati
- **Game dev** che vogliono capire un build già compilato
- **Studenti di system programming** che vogliono *vedere* concetti come cache miss, lock contention, scheduling

**NON è per**:

- Network analysis (Wireshark, ETW network providers ad-hoc esistono già)
- Security / forensics (la superficie di Argus non è progettata per resistere a target ostili)
- Profiling Linux / macOS (Windows-only per design)

## Principi guida

Quattro principi non negoziabili, in ordine di priorità:

### 1. Affidabilità prima di tutto

Argus **non deve crashare**. Mai. Né lui, né il processo target. Se non possiamo ottenere un dato, lo diciamo chiaramente all'utente e continuiamo. Vedi `06-reliability.md`.

### 2. Portatilità

Un singolo `.exe`, < 20 MB, nessuna dipendenza runtime, nessun installer. Si copia su una chiavetta e si esegue ovunque (Windows 10 1903+).

### 3. Overhead trascurabile

< 1% di overhead sul processo target, sempre misurabile e onesto. Se una metrica costa di più, è opt-in e l'utente lo sa.

### 4. Leggibilità

La dashboard deve essere comprensibile **dopo 30 secondi di osservazione** anche per chi non conosce ETW. Ogni grafico ha titolo, unità, e colorazione semantica. Tutte le metriche hanno una breve spiegazione accessibile (hover/tooltip).

## Criteri di successo

Argus è riuscito quando posso:

- ✅ Trovare in < 5 minuti la causa di un consumo CPU anomalo in un'app sconosciuta
- ✅ Identificare un memory leak osservando solo i grafici per 60 secondi
- ✅ Mostrarlo a un collega senza dover spiegare cosa significano le metriche
- ✅ Lasciarlo in esecuzione un'ora senza che cresca in RAM o lasci file pendenti
- ✅ Distribuirlo come singolo file zip < 15 MB

## Non-obiettivi (esplicitamente esclusi)

- **Cross-platform**: Windows-only per design. ETW e Win32 sono il cuore.
- **Real-time monitoring production-grade**: non è Datadog o Dynatrace, è un tool da workbench.
- **Modifica del processo target**: niente DLL injection, niente patching, niente code rewriting.
- **Tracing distribuito multi-process**: un solo processo alla volta (multi-process è una possibile Fase 4).
- **Plugin system**: Argus non ha plugin. È un binario chiuso, ben definito, affidabile.
- **Cloud / sync / telemetry**: niente network calls. Tutto locale, sempre.
