# 04 – Metriche

Questo documento è la **fonte di verità** per cosa Argus misura, da dove, e perché. Quando aggiungiamo una metrica, va prima descritta qui.

Ogni metrica ha:
- **Cosa è** (definizione)
- **Da dove** (API/provider)
- **Costo** (overhead stimato)
- **Cosa insegna** (concetto di ottimizzazione collegato)
- **Fase** in cui entra

## Fase 1 — Metriche polling-based (Win32)

Polling 10 Hz, overhead < 0.1% sul target.

### CPU %

- **Cosa**: percentuale di tempo CPU consumato dal processo, normalizzata sul numero di core logici. Range 0-100 (single-core saturato) o 0-N×100 se mostriamo per-core.
- **Formula**: `(Δ(kernel_time + user_time) / Δ wall_time) × 100 / num_cpus`
- **API**: `GetProcessTimes(handle)`
- **Costo**: ~1 µs per syscall
- **Insegna**: CPU-bound vs idle, parallelismo effettivo. Se vedi 25 % e hai 4 core, sei single-threaded.

### Working Set (RAM fisica)

- **Cosa**: memoria fisicamente residente in RAM per il processo.
- **API**: `GetProcessMemoryInfo` → `WorkingSetSize`
- **Insegna**: Working set che oscilla = paging attivo (sintomo di memory pressure). Crescita monotona = possibile leak.

### Private Bytes

- **Cosa**: memoria committed **non condivisa** con altri processi (heap, stack, dati statici).
- **API**: `GetProcessMemoryInfo` → `PagefileUsage`
- **Insegna**: differenza con Working Set = quanta memoria è pageata. Private crescente con WS stabile = leak che finisce in pagefile.

### I/O read / write rate (MB/s)

- **Cosa**: throughput di lettura e scrittura del processo (disk + named pipe + console).
- **API**: `GetProcessIoCounters` (delta `ReadTransferCount` / `WriteTransferCount` su Δt)
- **Insegna**: I/O bound vs CPU bound. Pattern burst (write batch) vs steady (logging).

### Thread count

- **Cosa**: numero di thread del processo.
- **API**: Toolhelp `TH32CS_SNAPPROCESS` → `PROCESSENTRY32W.cntThreads`
- **Insegna**: thread explosion (anti-pattern di app che spawna thread invece di usare pool).

### Handle count

- **Cosa**: numero di kernel handle aperti (file, registry, mutex, ...).
- **API**: `GetProcessHandleCount`
- **Insegna**: handle leak (crescita monotona = bug).

### Total I/O cumulativo

- **Cosa**: byte totali letti e scritti dall'avvio del processo.
- **API**: `GetProcessIoCounters` (valori assoluti)
- **Insegna**: utile per stimare costo I/O di un'operazione specifica osservando i delta.

## Fase 2 — Metriche ETW (kernel events)

Overhead variabile, target < 1 % con provider selezionati.

### CPU stack samples

- **Cosa**: ogni ~1 ms il kernel campiona il call stack di ogni thread runnable.
- **Provider ETW**: `PerfInfo` (alias `SystemTraceControlGuid`), evento `SampleProfile`
- **Aggregazione**: flame graph (tree con count per nodo)
- **Insegna**: hot path. **La** metrica fondamentale di un profiler.

### Context switch

- **Cosa**: ogni cambio di thread schedulato sulla CPU.
- **Provider**: `Thread`, evento `CSwitch`
- **Aggregazione**: thread states timeline (Running/Ready/Waiting/Blocked)
- **Insegna**: lock contention, oversubscription, priority issues, false sharing.

### Disk I/O dettagliato

- **Cosa**: ogni read/write sul disco, con file, offset, dimensione, durata.
- **Provider**: `DiskIo`
- **Aggregazione**: top files by I/O, latenza I/O (p50/p99)
- **Insegna**: pattern di accesso, file più contesi, latency outliers (HDD spindown, ssd thrashing).

### Page faults

- **Cosa**: ogni hard page fault (caricamento da disco).
- **Provider**: `PageFault`
- **Aggregazione**: rate, dove avvengono nel codice (stack del faulting thread)
- **Insegna**: working set sottodimensionato, mmap pattern, memory pressure.

### Heap allocations

- **Cosa**: ogni `HeapAlloc`/`HeapFree` (Windows heap).
- **Provider**: `Microsoft-Windows-Kernel-Memory` o `HeapTrace` (opt-in)
- **Aggregazione**: allocation by callstack, leak detection (long-lived allocations)
- **Insegna**: dove allochi, quanto vivono le allocazioni, allocation churn.

## Fase 3 — Metriche hardware

Richiede driver o uso di Intel VTune SDK / AMD uProf SDK. Da decidere.

### Cache miss / branch misprediction

- **Cosa**: contatori hardware delle CPU moderne (PMU - Performance Monitoring Unit).
- **API**: `NtSystemDebugControl` o driver dedicato
- **Insegna**: perché due funzioni "uguali" hanno performance diverse 10x. Cache locality, branch prediction.

### GPU usage

- **Cosa**: % utilizzo GPU, VRAM occupata, throughput PCIe.
- **API**: NVML (NVIDIA) / ADL (AMD) / `DXGI_QUERY_VIDEO_MEMORY_INFO`
- **Insegna**: bottleneck GPU vs CPU vs PCIe transfer cost.

### Power consumption

- **Cosa**: Watt consumati dal processo (su Windows 10+).
- **API**: `CallNtPowerInformation` con `ProcessorPowerInformation`
- **Insegna**: efficienza energetica, cost-per-operation.

## Convenzioni per le metriche

### Unità sempre esplicite

Mai `42.7` da solo nell'UI. Sempre `42.7 MB/s` o `42.7 %` o `42.7 / s`.

### Una metrica = un'unità

Non mischiare scale nello stesso grafico. Se serve, due grafici sovrapposti o assi separati.

### Colorazione semantica

- **Verde** (0-25 %): normale
- **Giallo** (25-60 %): attenzione
- **Rosso** (60-100 %): potenziale problema
- Mai dipendere **solo** dal colore (colorblind-safe).

### Storia

Default 60 secondi visibili. Lo stato interno può tenere più storia se cheap (counter cumulativi).

### Hover tooltip

Ogni grafico DEVE avere un tooltip che spiega in 1 riga cosa significa. Riusa il testo "Insegna" sopra.

### Naming

Italiano per label visibili all'utente ("Memoria", "Lettura disco"), inglese per identifier nel codice (`memory_working_set`, `disk_read_rate`).

## Metriche ETW profonde (Fasi 2/3) — implementate

Oltre al flame graph (hot path CPU) e alla timeline stati-thread:

- **Lock contention** (`aggregation/timeline.rs`). Dai `CSwitch` si legge la causa
  d'attesa `KWAIT_REASON` del thread uscente e la si attribuisce al segmento Waiting.
  `wait_category` la mappa in **Lock** (mutex/push lock/eventi/risorse → contesa),
  **IO** (paging/memoria), **UserIdle** (attesa volontaria / thread-pool a riposo),
  **Preempted**. `wait_breakdown[_of]` somma il tempo per categoria. *Insegna*: "Lock"
  alto = i thread si contendono sincronizzazione; "Idle" alto = sano. UI: riga "Attese
  per causa" + tooltip per segmento (es. `WrMutex`).
- **Disk I/O detail** (`capture/diskio.rs` + `aggregation/diskstats.rs`). Provider
  kernel `DiskIo`: per ogni operazione Read/Write completata → disco, byte, offset,
  tempo di risposta. Aggregati per direzione e per disco fisico (byte, n° operazioni,
  dimensione media). Eventi **di sistema** (non per-processo): misurano l'attività
  disco complessiva durante la cattura. *Insegna*: pattern di I/O (poche grandi vs
  molte piccole), quale disco è sotto pressione. *Rinviati*: nome file per operazione,
  percentili di latenza calibrati. UI: sezione "Disco fisico (ETW)" in Dashboard.
