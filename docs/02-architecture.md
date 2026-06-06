# 02 – Architettura

## Vista d'insieme

Argus è strutturato in **5 layer**, ciascuno con responsabilità ben definite e API pulite verso il layer sopra. Questo permette di sviluppare, testare e ottimizzare ogni livello in isolamento.

```
┌────────────────────────────────────────────────────┐
│ 5. UI Layer        (egui + eframe)                 │
│    pannelli, interazione utente, layout            │
├────────────────────────────────────────────────────┤
│ 4. Visualization   (wgpu compute + render)         │
│    flame graph, heat map, time series              │
├────────────────────────────────────────────────────┤
│ 3. Aggregation     (Rust pure)                     │
│    bucket, histogram, derivate, smoothing          │
├────────────────────────────────────────────────────┤
│ 2. Capture         (windows-rs + ETW)              │
│    polling, eventi kernel, ring buffer             │
├────────────────────────────────────────────────────┤
│ 1. Target Process  (the program being analyzed)    │
└────────────────────────────────────────────────────┘
```

Il dato fluisce dal basso verso l'alto: il processo target è osservato dal **Capture**, che produce sample/eventi che entrano nell'**Aggregation**, che produce strutture pronte per la **Visualization**, che alimenta l'**UI**.

## Threading model

> **Stato Fase 1**: il modello a 3 thread descritto sotto è il target finale. In
> Fase 1 il sampler produce già valori scalari, quindi sampler e aggregator sono
> **un unico thread**; l'aggregator dedicato entrerà con gli eventi ETW ad alta
> frequenza in Fase 2. Inoltre il conteggio thread è campionato a **1 Hz** (con
> carry-forward sulle storie a 10 Hz) per evitare uno snapshot Toolhelp ad ogni
> tick — un esempio dell'ottimizzazione che Argus stesso vuole insegnare.

Tre thread principali, comunicazione **lock-free dove possibile**:

### 1. Sampler thread (priority normale)
- Polling Win32 ogni 100 ms (10 Hz baseline)
- Riceve callback ETW (rate dipende dai provider; Fase 2+)
- Scrive in ring buffer SPSC verso l'aggregator

### 2. Aggregator thread (priority normale)
- Drena il ring buffer
- Aggiorna `Snapshot` immutable condiviso via `arc-swap`
- Calcola derivate (delta CPU time → %, byte → MB/s, ecc.)

### 3. UI thread (main, vincolato da eframe)
- Legge lo `Snapshot` più recente (immutable, zero-copy)
- Rendering wgpu @ 60 fps
- Gestisce input utente, manda comandi al sampler via MPSC channel

**Regola assoluta**: mai due thread scrivono allo stesso dato. La UI non aspetta mai il sampler.

## Moduli

**Decisione**: singolo crate con **lib + bin** — la logica vive nel lib (moduli pubblici), `main.rs` è un wrapper sottile, così i test d'integrazione in `tests/` la usano come `argus::…`. Workspace solo se la complessità lo giustifica (improbabile prima della Fase 3). Vedi `09-decisions.md` D12.

Struttura (attuale + pianificata):

```
src/
├── main.rs              # bin: entry sottile (logging + finestra)
├── lib.rs               # espone i moduli per i test (argus::…)
├── app.rs               # wiring: arc-swap + thread sampler + ciclo eframe
│
├── capture/             # LAYER 2 — osservazione del target
│   ├── mod.rs
│   ├── process.rs       # enum via NtQuerySystemInformation, attach/detach
│   ├── sampler.rs       # thread 10 Hz, comandi, pubblicazione Snapshot
│   ├── polling.rs       # (Fase 2) lettura metriche per-handle riusabile
│   ├── etw.rs           # (Fase 2) ETW session, provider, parsing eventi
│   └── symbols.rs       # (Fase 2) DbgHelp wrapper, simboli PDB
│
├── aggregation/         # LAYER 3 — stato time-series
│   ├── mod.rs           # Snapshot, storie, Status
│   └── flame.rs         # (Fase 2) stack samples -> flame graph
│
├── viz/                 # LAYER 4 — (Fase 2) pipeline wgpu custom
│   └── …                # flame view, heatmap
│
├── ui/                  # LAYER 5 — egui
│   ├── mod.rs           # State, render, top bar
│   ├── dashboard.rs     # KPI + grafici time-series
│   ├── process_list.rs  # picker (raggruppato, ordinabile)
│   └── kpi.rs           # card + grafici (egui_plot)
│
└── util/                # helper trasversali
    ├── error.rs         # ArgusError
    └── win.rs           # wrapper Win32 (RAII), SeDebugPrivilege

src/bin/fixture.rs       # carico deterministico per i test
tests/integration.rs     # test end-to-end del capture contro il fixture
```

> **Stato implementazione**: esistono `main.rs`, `lib.rs`, `app.rs`,
> `capture/{process,sampler}.rs`, `aggregation/mod.rs`,
> `ui/{mod,dashboard,process_list,kpi}.rs`, `util/{error,win}.rs`, più
> `src/bin/fixture.rs` e `tests/integration.rs`. I file marcati *(Fase 2)* e
> l'intero `viz/` non sono ancora creati.

## Flusso dati: vita di una metrica CPU (Fase 1)

1. **Sampler thread** ogni 100 ms chiama `GetProcessTimes(handle)` sul PID attaccato
2. Calcola delta kernel+user time vs campione precedente
3. Converte in % normalizzata sui core logici
4. Pusha in ring buffer SPSC
5. **Aggregator** drena, scrive in `Snapshot { cpu_history: VecDeque<f32> }`
6. **UI** legge `Snapshot`, passa il `VecDeque` al widget `LineChart`
7. `LineChart` (in `viz/`) genera vertex buffer + invia a wgpu
8. GPU disegna a 60 fps

Stessa pipeline per memoria, I/O, thread count.

## Flusso dati: vita di uno stack sample (Fase 2)

1. **ETW session** sottoscrive provider `PerfInfo` (sample profile)
2. Kernel scrive eventi in user-space buffer ETW
3. **Sampler thread** riceve callback: PID, TID, stack di ~40 indirizzi
4. Filtra per PID target, accumula
5. **Aggregator** chiama `symbols::resolve()` per ogni indirizzo nuovo (cache LRU)
6. Costruisce/aggiorna `FlameTree { root, children, count }`
7. **UI** legge `FlameTree`, `flame_view` lo renderizza con wgpu (un quad per nodo)

## Decisioni architetturali chiave

### Single binary statico

Tutto dentro l'eseguibile. Nessuna DLL plugin. Le PDB del processo target sono caricate via DbgHelp a runtime ma non distribuite con noi.

### Capture senza injection

**Mai modificare il processo target**. Solo:
- Win32 read-only API (`GetProcessTimes`, `GetProcessMemoryInfo`, `GetProcessIoCounters`)
- ETW session sottoscritta a provider già esistenti nel kernel
- Toolhelp snapshot per enumerazione

Conseguenza: niente line-level profiling né hook custom. Va bene per i nostri obiettivi (vedi `01-vision.md`).

### Storia dei dati: RAM only per Fase 1-2

- Time series: 10 Hz × 60 s = 600 sample per metrica → ~2.4 KB ciascuna
- Stack samples (Fase 2): 1 kHz × 60 s = 60k × 40 frame × 8 byte = ~19 MB
- **Allocazioni proprie** di Argus, stato stazionario: < 50 MB (Fase 1), < 150 MB (Fase 2-3). L'RSS *totale* del processo (~300 MB) è dominato dal working set del driver GPU/wgpu, non dalle nostre strutture — vedi `09-decisions.md`.

Fase 4 introdurrà capture-to-disk (formato `.argus` proprietario o `.etl`).

### Comunicazione thread

- **Sampler → Aggregator**: SPSC ring buffer **`crossbeam-channel::bounded`** (deciso in `09-decisions.md` D13; niente `rtrb` finché un profiling non lo giustifichi)
- **Aggregator → UI**: `arc-swap` di `Arc<Snapshot>` immutable
- **UI → Sampler** (commands): MPSC channel `crossbeam-channel::unbounded`

Mai lock sulla hot path di scrittura.

### Error model

Vedi `06-reliability.md`. In breve:
- Layer 1–3 ritornano `Result<T, ArgusError>`
- Layer 4–5 convertono in `Option` + stato UI
- Mai `?` propaga fino al main loop senza catturarlo nello state UI
