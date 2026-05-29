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

**Decisione provvisoria**: singolo crate, multi-module. Workspace solo se la complessità lo giustifica (improbabile prima della Fase 3).

Struttura prevista:

```
src/
├── main.rs              # entry point, eframe setup
├── app.rs               # state machine principale
│
├── capture/             # LAYER 2
│   ├── mod.rs
│   ├── polling.rs       # GetProcessTimes, GetProcessMemoryInfo...
│   ├── etw.rs           # ETW session, providers, parsing eventi (Fase 2)
│   ├── symbols.rs       # DbgHelp wrapper, simboli PDB
│   └── process.rs       # OpenProcess, attach/detach, ToolHelp
│
├── aggregation/         # LAYER 3
│   ├── mod.rs
│   ├── timeseries.rs    # ring buffer con storia
│   ├── flame.rs         # stack samples → flame graph (Fase 2)
│   └── delta.rs         # rate calculations
│
├── viz/                 # LAYER 4
│   ├── mod.rs
│   ├── line_chart.rs    # wgpu pipeline per time series
│   ├── flame_view.rs    # wgpu pipeline per flame graph
│   └── heatmap.rs       # wgpu pipeline per heat map
│
├── ui/                  # LAYER 5
│   ├── mod.rs
│   ├── dashboard.rs     # layout principale
│   ├── process_list.rs  # picker processi
│   ├── kpi.rs           # KPI cards
│   └── widgets/         # componenti riusabili
│
└── util/                # helper trasversali
    ├── error.rs         # ArgusError
    └── win.rs           # wrapper Win32 sicuri (RAII)
```

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
- Totale RAM target stato stazionario: < 100 MB

Fase 4 introdurrà capture-to-disk (formato `.argus` proprietario o `.etl`).

### Comunicazione thread

- **Sampler → Aggregator**: SPSC ring buffer (`crossbeam-channel::bounded` o `rtrb`)
- **Aggregator → UI**: `arc-swap` di `Arc<Snapshot>` immutable
- **UI → Sampler** (commands): MPSC channel `crossbeam-channel::unbounded`

Mai lock sulla hot path di scrittura.

### Error model

Vedi `06-reliability.md`. In breve:
- Layer 1–3 ritornano `Result<T, ArgusError>`
- Layer 4–5 convertono in `Option` + stato UI
- Mai `?` propaga fino al main loop senza catturarlo nello state UI
