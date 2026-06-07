# 09 – Decisioni e stato

Registro delle decisioni prese durante lo sviluppo (stile ADR, sintetico) e
fotografia del punto in cui siamo. Quando una scelta cambia, si aggiorna qui.

## Stato attuale

**Fase 1 (MVP polling) — completata.** Argus si attacca a un processo Windows e
ne mostra in tempo reale CPU, RAM (working set + private), I/O, thread e handle,
con dashboard GPU e lista processi raggruppata/ordinabile. Build, clippy e test
(2 unit + 3 integration) verdi.

**Fase 2 — ETW + flame graph: implementata, cattura live da verificare come
admin.** Fatto e testato (17 unit + 3 integration, clippy/fmt puliti): flame
graph puro (`aggregation/flame.rs`, D13), symbol resolution DbgHelp con cache
(`capture/symbols.rs`, D14), parser eventi + sessione ETW kernel
(`capture/etw.rs`, D15), aggregatore ETW→simboli→flame (`capture/profiling.rs`,
D16), tab "Flame graph" interattiva col Painter di egui (`ui/flame.rs`, D17).
L'app gira e mostra il degrado graceful "ETW non disponibile" senza admin
(verificato a video). **Cattura ETW live verificata come amministratore** (D20,
`tests/etw_live.rs`): 7690 stack reali dal fixture, ordine leaf-first. Restano i
nomi funzione del target (on-disk, D14), la misura overhead e la resa flame a
video — vedi handoff in [`07-roadmap.md`](07-roadmap.md).

**Fase 4 — recording/diff/export: completata.** Formato `.argus` (D18),
record/replay (D19), diff tra capture (`diff.rs`, tab Diff), export
CSV/folded/SVG (`export.rs`). Tutto testato (round-trip, export, diff).

**Fase 3 — timeline + lock + memoria: live e verificata.** Timeline stati-thread
(Running/Ready/Waiting) dai `CSwitch`, filtrata sui TID del target; **lock
contention** dalla causa d'attesa `KWAIT_REASON` (D21). **Disk I/O detail** (D22)
e **memoria** (hard fault + VirtualAlloc, D25) sullo stesso kernel logger, validati
live. Il tracking heap a livello `HeapAlloc` resta **fuori scope** per il vincolo
no-injection (D25).

**Overhead** misurato (D23): ~2.0–2.2% su loop CPU-bound stretto. **Infra**: export
JSON, CI, cargo-deny, tema (D24). **Latenza disco p50/p99** + **formato `.argus` v2**
(persiste disco/memoria/lock, retro-compatibile v1) + JSON completo (D26).

**Test totali**: 56 unit + 3 integration + 2 ignored (admin) verdi; clippy/fmt
puliti; release ~10.6 MB.

**Rilascio**: **v0.4.0** — prima release pubblica su GitHub (zip portatile +
`Install-Argus.ps1` per-utente; icona dell'app incorporata nell'exe, D27). Il
ramo `main` ora riflette l'intero lavoro Fasi 1-4 (riallineato dal branch
`feat/phase2-etw-flame`).

## Decisioni

### D1 — Linguaggio e stack: Rust + windows-rs + wgpu + egui
Niente GC/runtime (latenza prevedibile), binario singolo portabile, memory
safety su codice pieno di handle e puntatori. Dettagli e alternative scartate in
[`03-tech-stack.md`](03-tech-stack.md). **Conseguenza**: per alcune aree (ETW)
i binding sono meno maturi di .NET → scriveremo wrapper su misura in Fase 2.

### D2 — Nome: Argus
Il gigante dai 100 occhi della mitologia: osservatore totale. Binario `argus.exe`.

### D3 — Architettura a layer, ma 2 thread in Fase 1
Il target è 3 thread (sampler → aggregator → UI). In Fase 1 il polling produce
già valori scalari, quindi **sampler e aggregator sono un solo thread**;
l'aggregator dedicato arriverà con gli eventi ETW ad alta frequenza in Fase 2.
**Conseguenza**: meno complessità ora, senza precludere l'evoluzione.

### D4 — Pipeline lock-free: `arc-swap` + `crossbeam-channel`
Il sampler pubblica `Snapshot` immutabili via `ArcSwap`; la UI li legge senza
mai bloccarsi. I comandi UI→sampler (attach/detach/refresh) viaggiano su canale.
**Conseguenza**: la UI non aspetta mai il campionamento; clone dello snapshot
(~17 KB a 10 Hz) trascurabile.

### D5 — Conteggio thread a 1 Hz (carry-forward)
Le metriche O(1) (CPU, RAM, I/O, handle) sono campionate a 10 Hz; il numero di
thread arriva dall'enumerazione processi a 1 Hz e viene "tenuto" tra un refresh
e l'altro. **Conseguenza**: nessuna enumerazione costosa ad ogni tick — è
l'ottimizzazione che Argus stesso vuole insegnare.

### D6 — Backend wgpu in auto-selezione (non DX12 forzato)
Forzare DX12-only ha fatto **fallire la creazione della surface** sulla GPU di
test (RTX 5070 Ti, molto recente); Vulkan funzionava. Lasciamo che wgpu scelga
il backend disponibile (PRIMARY: Vulkan/DX12/…). **Conseguenza**: massima
portabilità; i warning Vulkan innocui sono silenziati nel log. Lezione: seguire
l'evidenza dell'hardware, non l'assunzione.

### D7 — Enumerazione processi via `NtQuerySystemInformation`
Per ordinare per uso di risorse serve CPU+RAM per **ogni** processo. Aprire 300
handle al secondo sarebbe spreco; `NtQuerySystemInformation(SystemProcessInformation)`
restituisce nome, sessione, RAM, tempi CPU, thread e handle di tutti i processi
in **una sola syscall** (come Task Manager / Process Explorer). **Conseguenza**:
overhead minimo; dipendenza da una API semi-documentata ma stabilissima.

### D8 — "Processi utente prima" via session ID
I processi della **sessione interattiva** di Argus (≈ avviati dall'utente) sono
raggruppati in cima; i servizi (sessione 0) sotto, attenuati. Ordine secondario:
CPU% / RAM / Nome, scelto in UI. **Conseguenza**: i tuoi programmi sono subito
in evidenza. Una distinzione più precisa per-utente (SID del token) è un
possibile affinamento futuro.

### D9 — Tempi CPU estratti dai byte `Reserved1`
windows-rs **non espone** `KernelTime`/`UserTime` come campi nominati di
`SYSTEM_PROCESS_INFORMATION`: vivono dentro `Reserved1` (layout stabile da
Vista — UserTime @offset 32, KernelTime @40). Li leggiamo con `from_le_bytes`.
**Conseguenza**: piccolo "hack" necessario, documentato e coperto da un test
d'integrazione che lo valida contro un carico reale.

### D10 — Attach con solo `PROCESS_QUERY_LIMITED_INFORMATION`
Niente `PROCESS_VM_READ`: il diritto limitato basta per tutte le query di Fase 1
ed è molto meno soggetto ad "accesso negato". **Conseguenza**: si attaccano molti
più processi utente senza privilegi elevati. `SeDebugPrivilege` viene tentato
all'avvio (utile solo se Argus gira come amministratore).

### D11 — Logging `tracing` filtrato per target
Default `warn,argus=info,wgpu_hal=error`: i log verbosi di wgpu non inondano il
file (`%LOCALAPPDATA%\Argus\argus.log`, rotazione giornaliera). Sovrascrivibile
via `RUST_LOG`. **Conseguenza**: log leggibile e utile per il debug.

### D12 — Struttura lib + bin per la testabilità
La logica vive in `lib.rs` (moduli pubblici); `main.rs` è un wrapper sottile.
Così i test d'integrazione in `tests/` usano `argus::...` e un binario
`src/bin/fixture.rs` con carico deterministico (brucia CPU, alloca, spawna
thread) fa da bersaglio reale. **Conseguenza**: validazione end-to-end del path
Win32 senza mock.

### D13 — Flame graph: albero puro keyed-by-nome, layout senza ricorsione
La struttura dati del flame graph (`aggregation/flame.rs`) è **pura**: aggrega
stack di *nomi di frame già risolti*, senza toccare Win32. Questo la disaccoppia
dal layer simboli (che mappa indirizzo→nome) e la rende interamente testabile
senza ETW né privilegi. Scelte:
- **Ordine stack root→leaf** (frame più esterno per primo): è l'ordine naturale
  del disegno; il layer ETW invertirà se serve.
- **Interning dei nomi** (`name_id: u32`) + una sola mappa `(genitore, name) →
  figlio` per tutto l'albero, invece di una HashMap per nodo: meno allocazioni.
- **Figli in ordine di prima comparsa**: stabile tra un update e l'altro, così i
  frame non saltano lateralmente mentre i contatori crescono live. Ordinamento
  per valore/alfabetico è un affinamento UI futuro.
- **Layout con work-stack esplicito** (niente ricorsione): robusto anche per
  stack patologicamente profondi (no-panic policy). Il focus si espande a piena
  larghezza con la catena di antenati per il drill-down/zoom.
**Conseguenza**: il rendering (egui o wgpu) consuma solo `layout(focus) →
Vec<Rect>` + accessor di lettura; nessuna logica di profiling nella UI.

### D14 — Symbol resolution: DbgHelp RAII, cache a 2 generazioni, fallback
`capture/symbols.rs` avvolge DbgHelp (`SymInitializeW`/`SymFromAddrW`/
`SymGetModuleInfoW64`/`SymCleanup`) in un tipo RAII. DbgHelp **non è
thread-safe**: il resolver è posseduto da un solo thread (l'aggregatore).
Scelte:
- **Mai panic, mai nome inventato**: se la risoluzione fallisce si ripiega su
  `modulo!0xADDR` o `0xADDR` (graceful degradation, docs/06-reliability.md).
- **Cache a due generazioni** (hot/cold) invece di un LRU con liste intrusive:
  memoria limitata a ~2×cap, O(1), gli indirizzi caldi sopravvivono alla
  rotazione. Niente dipendenza `lru`.
- **Risoluzione del target**: la strategia definitiva sarà *on-disk* — caricare
  i moduli (`SymLoadModuleExW`) dai path/base degli eventi ETW Image/Load,
  invece di leggere la memoria del target vivo. Più robusto (funziona anche dopo
  l'uscita del processo) e non richiede `PROCESS_VM_READ`, mantenendo l'attach
  minimale di D10. Il resolver è comunque già in grado di operare su un handle
  vivo (`invade = true`), come fanno i test che risolvono sé stessi.
**Conseguenza**: l'attach di Fase 1 resta invariato; la decisione su come/quando
aprire i moduli del target si concretizza con la sessione ETW.

### D15 — Sessione ETW: NT Kernel Logger, real-time, consumer thread, drop-on-full
La cattura degli stack sample (`capture/etw.rs`, `EtwProfiler`) usa la sessione
kernel classica "NT Kernel Logger": `StartTraceW` con `EVENT_TRACE_FLAG_PROFILE`
+ `TraceSetInformation(TraceStackTracingInfo)` per lo stack-walk dell'evento
`SampleProfile`, consumata in real-time da un thread dedicato (`ProcessTrace`).
Scelte:
- **Degrado graceful**: senza admin `StartTraceW` dà `ACCESS_DENIED` →
  `start()` ritorna `Err(Permission)` con suggerimento; Argus prosegue in
  polling-only (no panic, no retry-loop). Coperto da test (no-admin).
- **`try_send` nel callback**: il callback ETW gira sul consumer del kernel e
  **non deve mai bloccare**; in overflow del canale (bounded) lo stack si scarta.
- **Stop pulito**: `CloseTrace` sblocca `ProcessTrace`, poi `ControlTraceW(STOP)`
  e join del thread (RAII su Drop).
- **Parsing isolato e testato**: il decode binario (`parse_stack_walk`) è puro e
  coperto da unit test con buffer sintetici (la parte più bug-prone).
- **Verifica**: la cattura *live* richiede admin e **non è verificabile nei test
  non elevati** — va collaudata a mano come amministratore (vedi `07-roadmap.md`
  DoD Fase 2). Il codice compila, l'`unsafe` è isolato/commentato e il path di
  fallback è testato.

### D16 — Flame graph condiviso via `Mutex`, non arc-swap (deviazione mirata da D4)
La pipeline ad alta frequenza (polling → Snapshot) resta lock-free via `arc-swap`
(D4 invariato). Il **flame graph**, invece, è condiviso UI↔aggregatore con un
`Arc<Mutex<FlameGraph>>`. Motivo: è un albero **mutato di continuo** (un
`add_stack` per sample); pubblicarne un clone immutabile via `arc-swap` ad ogni
update costerebbe O(nodi) con molte allocazioni, mentre il dato è a frequenza
più bassa (limitato dalla risoluzione simboli) e letto dalla UI a ~30 fps. Le
sezioni critiche sono brevissime: l'aggregatore risolve **fuori** dal lock e lo
prende solo per gli `add_stack` in batch; la UI lo prende solo per calcolare il
`layout`. Contesa trascurabile. **Futuro**: se emergessero stalli UI, si passerà
a pubblicare uno snapshot immutabile *render-only* (`FlameView`) via arc-swap.
**Conseguenza**: meno codice e nessun clone costoso ora, senza toccare la
garanzia lock-free della hot path di Fase 1.

### D17 — Flame graph renderizzato col Painter di egui (non pipeline wgpu custom)
La roadmap prevedeva un renderer wgpu **custom** (un quad per nodo). Per la prima
versione disegniamo invece i rettangoli col `Painter` di egui (`ui/flame.rs`).
Motivi: per il numero di nodi in gioco (migliaia) egui è già performante e
affidabile; è codice molto più semplice e — soprattutto — **verificabile
eseguendo l'app** (la cattura ETW richiede admin, ma il rendering no). **Non è un
cambio di stack**: egui disegna comunque via wgpu sotto, quindi non ricade nel
divieto di cambiare stack senza discussione. Il renderer wgpu custom resta
un'ottimizzazione futura, sensata solo per grafi enormi (>10⁵ nodi) o effetti
particolari. **Conseguenza**: tab Flame interattiva (zoom/drill, ricerca, hover)
con poco codice; `viz/` non è ancora necessario.

### D18 — Formato `.argus`: binario manuale, versionato, zstd rinviato
La persistenza di sessione (Fase 4, `persist.rs`) usa un formato binario
little-endian con magic header `ARGUSCAP` + versione (`u16`) + byte di
compressione. Scelte:
- **Serializzazione manuale**, niente `serde`/`bincode`: i dati sono semplici
  (scalari, `Vec<f32>`, albero flame), il round-trip è interamente testabile e
  non aggiungiamo dipendenze di serializzazione (disciplina sulle dipendenze).
- **Letture bounds-checked** via `util::bytes::ByteReader` (ritorna `Option`):
  mai panic su file troncato o corrotto (no-panic policy). Pre-alloc limitata ai
  byte disponibili → niente OOM su conteggi falsificati.
- **Flame** serializzato con tabella nomi deduplicata + nodi piatti, ricostruito
  con `FlameGraph::from_nodes` (valida i genitori, ignora i riferimenti errati).
- **Compressione zstd rinviata**: aggiungerebbe `zstd-sys` (dipendenza C) per
  file < 1 MB. Il byte `compression` nell'header permette di introdurla come
  nuova versione del formato senza rotture.
**Conseguenza**: record/replay senza nuove dipendenze pesanti, formato evolvibile.

### D19 — Record/replay: auto-path + lista in-app, niente file dialog nativo
Salva/apri sessione (Fase 4) senza dialog nativo:
- **Salva**: file `.argus` auto-nominato (`<processo>-<epoch>.argus`) in
  `%LOCALAPPDATA%\Argus\captures`. **Apri**: lista in-app dei `.argus`
  (scansionata dal sampler, pubblicata via `ArcSwap<Vec<PathBuf>>`). Evita la
  dipendenza `rfd` o codice `unsafe` su comdlg32. Il dialog nativo resta una
  nicety futura.
- **Replay**: nuovo `Status::Replay`; il sampler inietta lo `Snapshot`
  ricostruito (`Capture::to_snapshot`) e il flame caricato, e non li sovrascrive
  (handle `None`; il controllo "target uscito" è ora gated su handle live, così
  il replay non viene scambiato per un processo terminato).
- **notice**: campo transitorio nello `Snapshot` per il feedback UI
  (salvato/caricato/errore).
**Conseguenza**: record/replay completo, zero nuove dipendenze.

### D20 — La sessione ETW PROFILE richiede `SeSystemProfilePrivilege` abilitato
Scoperto col **test live elevato** (`tests/etw_live.rs`): da amministratore,
`StartTrace` del kernel logger con `EVENT_TRACE_FLAG_PROFILE` ritornava **1314
(ERROR_PRIVILEGE_NOT_HELD)**. *Avere* il privilegio (come admin) non basta: va
**abilitato** nel token via `AdjustTokenPrivileges`, come `SeDebugPrivilege`.
Fix: `enable_privilege(name)` generico in `util/win.rs`, e `EtwProfiler::start`
chiama `enable_privilege("SeSystemProfilePrivilege")` prima di `StartTrace` (e
mappa 1314 → `Permission`). **Verificato end-to-end** (run elevato): 7690 stack
reali catturati dal fixture, tutti del PID target, profondità fino a 99, flame
costruito. **Ordine confermato leaf-first** (`frames[0]`=foglia,
`frames[ultimo]`=`ntdll!RtlUserThreadStart`) → il `.rev()` in `profiling.rs` è
corretto, flame orientato bene. Simboli: moduli di sistema risolti coi nomi
(ntdll/kernel32); i nomi funzione del *target* richiedono l'approccio on-disk
(D14) — ora si vede `modulo!0xADDR` (degrado graceful).
**Conseguenza**: la cattura ETW live funziona da admin; coperta da un test
d'integrazione `#[ignore]`d ri-eseguibile elevato.

### D21 — Lock contention via `KWAIT_REASON`, non un provider extra
I `CSwitch` portano già `OldThreadWaitReason` (parsato in `cswitch.rs` ma prima
scartato). Invece di aggiungere un provider, lo **propaghiamo** ai segmenti
Waiting della timeline e lo **categorizziamo** (`wait_category`: Lock/IO/UserIdle/
Preempted) dai valori dell'enum NT `KWAIT_REASON`. `wait_breakdown` quantifica la
contesa; la UI Timeline mostra "Attese per causa" + tooltip per segmento. Costo
quasi nullo, nessun evento in più. Verificato live (1535 CSwitch).

### D22 — Disk I/O detail sullo stesso kernel logger (provider `DiskIo`)
Aggiunto `EVENT_TRACE_FLAG_DISK_IO` alla sessione esistente (non una nuova
sessione): `capture/diskio.rs` decodifica `DiskIo_TypedData` (TransferSize@8,
ByteOffset@16, HighResResponseTime dopo i due puntatori), `aggregation/diskstats.rs`
aggrega per direzione e per disco. Gli eventi sono **di sistema** (niente PID nel
payload): misurano l'attività disco complessiva durante la cattura — etichettato
così nella UI. **Layout validato live**: una scrittura-probe di 16 MB dà 16.4 MB
in `diskstats.write` → offset corretti. *Rinviati*: nome file per operazione
(serve provider `FileIo` + correlazione `FileObject`) e percentili di latenza
calibrati (serve la frequenza QPC). Heap allocations invece **richiederebbe un
secondo tipo di sessione** ETW → rinviato come aggiunta architetturale separata.

### D23 — Overhead misurato come dilatazione a lavoro fisso
`fixture bench` esegue un lavoro deterministico single-thread (non a tempo) e
stampa l'elapsed; `tests/overhead.rs` (#[ignore], admin+release) confronta la
mediana con/senza sessione ETW attiva. Il sampling kernel è system-wide, quindi
il filtro per PID non cambia il costo: misura reale. Risultato: **2.21%** su un
loop CPU-bound stretto — caso peggiore per il sampling (massima frequenza di
interruzioni); il target <1% di `01-vision` vale per carichi reali con attese.

### D24 — JSON/CI/cargo-deny sì, zstd/PNG no (disciplina dipendenze)
Export **JSON** scritto a mano (niente serde): dati semplici, nessuna dipendenza.
**CI** GitHub Actions (windows: fmt/clippy/test/build) + **cargo-deny** (licenze/
advisory) come da `03-tech-stack`. **Non** aggiunti: compressione zstd del `.argus`
(file minuscoli, `zstd-sys` introduce una libreria C → supply-chain; D18 lo
rinviava già, header forward-compatible) ed export PNG (ridondante con l'SVG già
condivisibile, `image` è un albero deps grande). Coerente con "no dipendenze
comode" di `CLAUDE.md`.

### D25 — Memoria: hard fault + VirtualAlloc, non heap-level (vincolo no-injection)
Il provider `PageFault` sul kernel logger dà due segnali catturabili su un
processo **già in esecuzione senza modificarlo**: hard page fault (opcode 32,
flag `MEMORY_HARD_FAULTS` — page-in da disco, filtrati sui TID del target) e
VirtualAlloc/VirtualFree (opcode 98/99, flag `VIRTUAL_ALLOC` — riserve di VM,
filtrate sul PID nel payload). Si abilitano solo questi flag (no soft fault, che
sono altissima frequenza → flood). `capture/memevents.rs` + `aggregation/memstats.rs`.
**Validato live**: con il fixture che fa churn di blocchi da 16 MB durante la
cattura → 19 VirtualAlloc = 304 MB, granularità 16 MB. Il tracking a livello
**`HeapAlloc`** (per-allocazione) **non è fattibile** rispettando il vincolo
non-negoziabile di Argus ("attach a un processo in esecuzione senza injection né
modificarlo", `CLAUDE.md`): la tracciatura heap di Windows richiede che il target
abbia il tracing abilitato **al lancio** (IFEO `TracingFlags`, o `tracelog -heap`),
cosa impossibile da attivare retroattivamente su un processo arbitrario già avviato
senza iniettare codice o rilanciarlo. VirtualAlloc (granularità di pagina) è
l'alternativa compatibile che forniamo; heap-level resta fuori scope by design.

### D26 — `.argus` v2: persiste le metriche ETW profonde; latenza disco p50/p99
Due completamenti di deliverable previsti:
- **Latenza disco**: `DiskStats` tiene un istogramma log2 dei tempi di risposta
  (memoria costante, O(1) insert) → `p50`/`p99` raw; `util/win::qpc_frequency`
  (cache `OnceLock`) calibra i tick QPC del trace in **ms**. UI: riga "Latenza per
  operazione" nella sezione Disco. Resta solo il **nome file** per operazione
  (correlazione `FileObject`→nome via provider `FileIo`).
- **Formato `.argus` v2**: `Capture` ora porta `wait` (`WaitBreakdown`), `mem`
  (`MemStats`) e `disk` (`DiskSnapshot`, proiezione read-only serializzabile),
  scritti in coda al flame. La **lettura accetta anche v1** (campi nuovi → default):
  backward-compatible. L'export **JSON** include `wait`/`memory`/`disk`. Coperto da
  test (round-trip v2, lettura v1, JSON). **Nota**: il *replay a video* di queste
  metriche non è ancora cablato (la UI di replay mostra metriche+flame; i dati
  profondi sono nel file e nel JSON, ma per mostrarli a schermo va ripopolato
  `shared.disk/mem` in replay) — vedi `07-roadmap.md`.

### D27 — Icona dell'app: occhio "all-seeing" generato e incorporato nell'exe
L'identità visiva è un **occhio** (Argus, il gigante dai cento occhi) con iride
violetta — la stessa `ACCENT` di `ui/theme.rs` (#7B61FF) su tile scura — dentro
un reticolo di misura (richiamo al profiling). Sorgente vettoriale in
`assets/argus.svg`; la `.ico` multi-size (16→256) è generata da quella e
**incorporata nell'eseguibile** via `build.rs` + `winresource` (build-dependency,
solo Windows): appare in Explorer, taskbar e finestra. Se manca il resource
compiler la build **non fallisce** (solo warning), coerente con la degradazione
con grazia. **Conseguenza**: nessuna dipendenza runtime aggiunta; l'icona vive nel
sorgente SVG ed è rigenerabile.

### D28 — Dashboard ridisegnata: hero CPU, sparkline, grafici ad area, griglia responsive
La pagina Metriche è stata rifatta per dare gerarchia e leggibilità
(`05-ui-design`): card **hero** per la CPU (valore grande + sparkline ampia), card
compatte con **mini-sparkline** del trend, grafici ad **area** su due colonne con
valore corrente e tooltip su hover. Le card stanno in una **griglia responsive**
(numero di colonne in base alla larghezza, righe bilanciate 6/3/2/1) che riempie
lo spazio invece di lasciarlo morto; anche le sezioni Disco/Memoria (ETW) usano
colonne uniformi. **Conseguenza**: si capisce a colpo d'occhio, **nessuna nuova
dipendenza** (sparkline disegnata col painter, area con `egui_plot`).

### D29 — Repaint adattivo (meno CPU/GPU a riposo)
La UI ridisegnava a 30 fps fissi *sempre*, anche da ferma, pur con dati a ≤10 Hz e
repaint immediato su input: spreco. Ora la cadenza è adattiva — **33 ms** se
attiva (collegata/replay), **400 ms** da ferma/non collegata, **1 s** minimizzata.
**Conseguenza**: meno CPU/GPU quando Argus sta in background, senza perdere
fluidità durante il profiling. Trovato col **dogfooding** (Argus che esamina
Argus); un monitor di ~8 min ha inoltre confermato **nessun leak** di RAM/handle/
thread in cattura attiva (RAM ~318 MB stabile, dominata dal driver GPU/wgpu).

## Questioni aperte

- **Budget RAM**: a riposo Argus usa ~304 MB, sopra il target di 300 MB scritto
  nei docs. Quasi tutto è overhead del driver GPU/wgpu (le strutture dati di
  Argus sono <1 MB). Da decidere: rivedere il budget o misurare separatamente la
  "RAM nostra".
- ~~**Dimensione binario release**: da misurare contro il target <15 MB.~~
  **Risolto**: 10.62 MB in release (Fase 2), ben sotto il target.
- **Edge case di affidabilità** non ancora testati in modo dedicato: GPU device
  lost, sistema low-memory (vedi tabella in [`06-reliability.md`](06-reliability.md)).
