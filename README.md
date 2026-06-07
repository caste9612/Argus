# Argus

**Profiler portatile e didattico per Windows.**
Attàccati a un programma in esecuzione e *guarda* — e impara a capire — come usa CPU, memoria, I/O, thread e lock.

> ✅ **Stato:** Fasi 1-4 complete (validate live, da amministratore): metriche real-time, **flame graph** (ETW), **timeline** dei thread + **lock contention**, **memoria** (hard page fault / VirtualAlloc) e **disco** con latenze p50/p99, **registrazione/replay** `.argus`, **diff** tra sessioni ed **export** CSV/SVG/JSON. Overhead misurato **~2%**. Il profiling completo (flame/timeline/disco/memoria) **richiede l'avvio come amministratore** (ETW). Come si usa: scheda **Guida** nell'app o [`docs/10-uso.md`](docs/10-uso.md); dettagli in [roadmap](docs/07-roadmap.md), [decisioni](docs/09-decisions.md), [backlog](docs/11-backlog.md).

## Perché Argus

Quasi tutti gli strumenti ti *mostrano* dei numeri. Argus vuole anche **insegnarti a leggerli**: capire *perché* un programma è lento, *dove* spende il tempo, se perde memoria, se litiga sui lock. Si attacca a un processo **già in esecuzione, senza modificarlo** (niente injection, niente hook) e mostra tutto in tempo reale con una dashboard GPU che regge milioni di sample senza scatti.

È nato con uno **scopo didattico**: non solo vedere CPU/RAM/I/O, ma capire *come si comporta* un software sotto il cofano. Per questo ogni metrica ha un **tooltip** che spiega cosa misura e cosa indica un valore anomalo, e dentro l'app c'è una scheda **Guida** che spiega come usarla e come interpretare i dati.

## Cosa puoi imparare

- **Dove va il tempo (CPU-bound)** — il *flame graph* mostra le funzioni che consumano CPU: barra larga = tanto tempo. Trovi l'hot path senza leggere stack trace a mano.
- **Aspetta o calcola? (I/O / lock-bound)** — la *timeline* dei thread distingue **Running / Ready / Waiting**; se un thread resta in attesa su un **lock**, lo vedi (contesa) e capisci che il collo di bottiglia non è la CPU.
- **Memory leak** — i *private bytes* che crescono in modo monotono = memoria che non viene mai rilasciata.
- **Paging** — molti *hard page fault* = il working set non entra in RAM e il sistema pagina da disco → rallentamenti improvvisi.
- **Leak di risorse** — *handle* che salgono senza mai scendere = handle (file, socket, …) non chiusi.
- **Disco sotto pressione** — latenza **p99** molto più alta della mediana = code di I/O occasionali.

> In breve: Argus trasforma concetti astratti ("è lento", "consuma troppo") in qualcosa che **vedi e sai spiegare**.

## Obiettivi (non negoziabili)

- **Portatile** — un singolo `.exe`, niente runtime, niente installazione obbligatoria
- **Affidabile** — niente crash, niente panic, degrado graduale se mancano permessi o dati
- **Leggibile** — grafica curata, dati comprensibili a colpo d'occhio
- **Didattico** — non solo numeri: ti aiuta a *capirli*

## Per chi è

Sviluppatori (Rust / C++ / .NET / Go / …) e curiosi che vogliono capire cosa sta facendo un loro programma in produzione o in test — **senza invocare `perfview` con 17 flag** né leggere stack trace di 200 righe.

## Download

Scarica l'ultima versione dalla pagina **[Releases](https://github.com/caste9612/Argus/releases)**:

- **`argus-vX.Y.Z-win-x64.zip`** — versione **portatile**: estrai ed esegui `argus.exe`, nessuna installazione (fedele alla filosofia di Argus).
- Nello zip c'è anche **`Install-Argus.ps1`**: tasto destro → *Esegui con PowerShell* per installare Argus per l'utente corrente (copia in `%LOCALAPPDATA%\Programs\Argus` + collegamento nel menu Start, **senza privilegi admin**).

Requisiti: **Windows 10/11 a 64 bit**. Per il profiling completo (flame graph, timeline, disco, memoria) avvia Argus **come amministratore** (richiesto da ETW); le metriche base (CPU/RAM/I/O/thread/handle) funzionano anche senza.

## Come si usa (in breve)

1. Avvia Argus — **come amministratore** per sbloccare flame graph, timeline, disco e memoria (ETW).
2. Scegli un processo nella lista a sinistra → **Collega** (o doppio click).
3. Esplora le schede:
   - **Metriche** — CPU, RAM (working set + private), I/O, thread, handle in tempo reale, ognuno con sparkline e tooltip.
   - **Flame graph** — dove il processo spende tempo CPU (zoom, ricerca regex).
   - **Timeline** — stati dei thread nel tempo e attese per causa (lock/IO/idle).
   - **Diff** — confronta due sessioni (prima/dopo un'ottimizzazione).
4. **Salva** la sessione in un file `.argus`, riaprila in *replay*, **confrontala** o **esporta** (CSV / SVG del flame / JSON completo).

Guida completa e interpretazione dei dati: scheda **Guida** nell'app, oppure [`docs/10-uso.md`](docs/10-uso.md).

## Build da sorgente

```powershell
cargo run --release        # build ottimizzato ed esegui
cargo test                 # unit + integration
cargo clippy --all-targets -- -D warnings
```

## Documentazione

La progettazione vive in [`docs/`](docs/) — leggi in ordine per capire il progetto:

1. [Visione](docs/01-vision.md) — problema, obiettivi, principi
2. [Architettura](docs/02-architecture.md) — com'è strutturato il sistema
3. [Tech stack](docs/03-tech-stack.md) — le scelte tecnologiche e perché
4. [Metriche](docs/04-metrics.md) — cosa misuriamo, da dove, **cosa insegna**
5. [UI design](docs/05-ui-design.md) — linguaggio visivo
6. [Affidabilità](docs/06-reliability.md) — garanzie e strategie
7. [Roadmap](docs/07-roadmap.md) — fasi e milestone
8. [Sviluppo](docs/08-development.md) — workflow di lavoro
9. [Decisioni](docs/09-decisions.md) — log delle scelte e stato attuale
10. [Uso](docs/10-uso.md) — **come si usa Argus e come leggere i dati**
11. [Backlog](docs/11-backlog.md) — feature/implementazioni mancanti

Chi collabora (umani e AI) legga **`CLAUDE.md`** in radice per le convenzioni.

## Stack tecnologico

- **Rust** 1.92+ — no GC, no runtime, binario singolo
- **windows-rs** — API Win32 native + ETW (Event Tracing for Windows)
- **wgpu** — rendering GPU moderno (backend auto-selezionato: Vulkan/DX12)
- **egui / eframe** — UI immediate-mode

Dettagli e alternative scartate: [docs/03-tech-stack.md](docs/03-tech-stack.md).

## Licenza

MIT — vedi [LICENSE](LICENSE).
