# 10 – Spike ETW (Fase 2)

Documento di lavoro dello **spike** che apre la Fase 2: validare la cattura
kernel-level via ETW in `windows-rs` raw *prima* di costruirci sopra il flame
graph. Vedi [`07-roadmap.md`](07-roadmap.md) (Fase 2) e
[`09-decisions.md`](09-decisions.md) D14.

## Perché uno spike

ETW kernel (NT Kernel Logger + provider `PerfInfo`) è il pezzo a più alto
rischio della Fase 2: binding `windows-rs` meno maturi, `unsafe`, privilegi
admin, sessione di sistema unica. Lo spike risponde a tre domande **prima** di
investire nel resto (aggregator, flame tree, renderer wgpu):

1. I binding ETW esistono e si allineano in `windows-rs` 0.58? → **sì** (M0).
2. Riusciamo a ricevere stack sample filtrati per PID? → da validare (M2-M3).
3. L'overhead sul target resta < 1%? → da validare (M4).

## Milestone

| # | Cosa | Stato |
|---|---|---|
| M0 | Control path: `StartTraceW`/`ControlTraceW`/`TraceSetInformation`, `EVENT_TRACE_PROPERTIES`, GUID/flag — compila, clippy-clean, e fallisce con grazia senza admin (`ERROR_ACCESS_DENIED` → messaggio chiaro, no panic) | ✅ |
| M1 | La sessione **parte** da un terminale amministratore | ⏳ run elevato |
| M2 | Arrivano eventi di stack walk dal provider profile | ⏳ |
| M3 | Filtro per il PID target → `StackSample` accumulati | ⏳ |
| M4 | Overhead sul target misurato < 1% (fixture con/senza sessione) | ⏳ |

M0 è verificato in automatico (compile + clippy + run non-elevato). M1-M4
richiedono privilegi di amministratore e vengono eseguiti a mano.

## Come eseguire

Da un terminale **amministratore** (la NT Kernel Logger è una sessione di
sistema unica e richiede privilegi elevati):

```powershell
cargo run --features etw --bin etw_spike -- <pid>
```

Atteso oggi (M0/M1): "sessione avviata", 3 secondi di attesa, poi stop pulito.
Senza admin: "Sessione NON avviata: … accesso negato …" ed exit 1 (no panic).

## Cosa è implementato

- `src/capture/etw.rs` — `KernelTraceSession`:
  - `start()`: costruisce `EVENT_TRACE_PROPERTIES` in un buffer allineato a 8
    byte, imposta `SystemTraceControlGuid` + `EVENT_TRACE_FLAG_PROFILE` +
    `EVENT_TRACE_REAL_TIME_MODE`, chiama `StartTraceW(KERNEL_LOGGER_NAMEW)`.
  - `set_sampling_interval()`: `TraceSetInformation(TraceSampledProfileIntervalInfo)`
    a ~1 kHz (10000 × 100 ns).
  - `Drop`: `ControlTraceW(EVENT_TRACE_CONTROL_STOP)` (teardown RAII).
  - `StackSample { pid, tid, frames }`: tipo di output (popolato dal consumer).
- `src/bin/etw_spike.rs` — harness CLI per la validazione elevata.
- Tutto dietro la feature opt-in `etw` (default build invariato — vedi D14).

## Prossimi step — il consumer real-time

Dal `TODO(spike)` in `etw.rs`, da implementare e validare con admin:

1. **Stack walk on-event**: `TraceSetInformation(handle, TraceStackTracingInfo,
   &CLASSIC_EVENT_ID { EventGuid: SampledProfile, Type: 46 })` per agganciare lo
   stack al profile event.
2. **Logfile real-time**: `EVENT_TRACE_LOGFILEW` con `LoggerName`,
   `ProcessTraceMode = REAL_TIME | EVENT_RECORD`, `EventRecordCallback`.
3. `OpenTraceW` → `PROCESSTRACE_HANDLE` (verificare `INVALID_PROCESSTRACE_HANDLE`).
4. Thread dedicato: `ProcessTrace(&[h], None, None)` (blocca finché la Drop ferma
   la sessione → join pulito).
5. Callback `extern "system" fn(*mut EVENT_RECORD)`: filtra
   `EventHeader.ProcessId == target`, accumula gli `StackWalk` in `StackSample`.
   Il callback non può catturare stato → contatori/coda globali (`AtomicU64` +
   canale `crossbeam` verso l'aggregator, coerente con la pipeline di Fase 1).

Poi: risoluzione simboli (`capture/symbols.rs`, DbgHelp + cache LRU) e flame tree
(`aggregation/flame.rs`) — ma solo dopo che M2-M4 sono verdi.

## Note tecniche e rischi

- **Sessione unica**: se un'altra NT Kernel Logger è già attiva, `StartTraceW`
  ritorna `ERROR_ALREADY_EXISTS` (oggi mappato su `Internal`). Da gestire con un
  "stop preventivo" o un nome di sessione privato (Win8+: system trace privata).
- **Parsing StackWalk**: il layout dell'evento `StackWalk` va decodificato dai
  `EVENT_RECORD` (numero di frame variabile). Sarà la parte più fragile.
- **Overhead (M4)**: misura confrontando il fixture (`src/bin/fixture.rs`) con e
  senza sessione attiva — è il test che dà senso al claim "< 1%".
- **`ferrisetw` vs raw**: la scelta di produzione si fa **dopo** lo spike (D14).
  Se il consumer raw risulta troppo verboso e `ferrisetw` copre il caso, si
  valuta lì — non prima.
