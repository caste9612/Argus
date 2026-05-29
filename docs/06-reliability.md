# 06 – Affidabilità

L'affidabilità è il **principio #1** di Argus (vedi `01-vision.md`). Questo documento definisce cosa significa concretamente e come la garantiamo.

## Definizione

Argus è **affidabile** quando:

1. **Non crasha mai**, in nessun input plausibile o ostile
2. **Non rallenta il processo target oltre l'1 %**, sempre
3. **Non corrompe né altera** il processo target, mai
4. **Comunica chiaramente** quando un'operazione non riesce e perché
5. **Degrada con grazia**: se una metrica non si può ottenere, le altre continuano

## No-panic policy

Il binario in release deve essere immune da panic reachable. Ogni `panic!` raggiungibile è un bug **critico**.

### No-panic guarantees essenziali

- Mai `.unwrap()` su `Result`/`Option` da API esterna (Win32, ETW, file, …)
- Mai indexing `[i]` su slice/vec senza bound check esplicito o `get(i)`
- Mai divisione per zero senza check (importante per delta time con system suspend)
- Mai cast `as` che possono troncare valori importanti — preferire `try_into()`

### Tooling per applicare la policy

- `cargo clippy -- -W clippy::unwrap_used -W clippy::panic -W clippy::expect_used`
- Code review: ogni `unwrap` aggiunto richiede commento di giustificazione
- (Futuro) crate `no_panic` come marker su funzioni critiche

## Error handling

### Tipi di errore

```rust
pub enum ArgusError {
    Os(windows::core::Error),       // errori Win32/ETW
    Symbol(SymbolError),            // PDB/DbgHelp
    NotAttached,                    // operazione richiede attach
    Permission { hint: String },    // access denied + suggerimento
    Internal(String),               // bug interno (loggato + non-fatale)
}
```

Implementare `Display` con messaggi user-friendly in italiano per l'utente finale.

### Propagazione

- Layer 1–3 (capture, aggregation): `Result<T, ArgusError>`
- Layer 4–5 (viz, ui): convertono in `Option<T>` con stato visualizzato
- Mai `?` propaga fino al main loop senza catturarlo nello state UI

### Logging

`tracing` crate, livelli:

- `error!` per failures che bloccano una funzionalità
- `warn!` per recuperabili
- `info!` per eventi importanti (attach, detach, ETW session start)
- `debug!` per developer

Default sink: file `%LOCALAPPDATA%\Argus\argus.log`, rotato a 10 MB, max 5 file.

## Edge cases obbligatori

Ognuno DEVE essere gestito con test esplicito prima della release fase corrispondente:

| Caso | Comportamento atteso | Fase |
|---|---|---|
| Target exit durante sampling | Detach automatico + UI state "process exited" | 1 |
| OpenProcess Access Denied | Errore con suggerimento "run as admin" | 1 |
| OpenProcess su PID inesistente | Errore "process not found" | 1 |
| Target è protetto (PPL/AV) | Errore con spiegazione, no retry loop | 1 |
| Target a 32-bit (siamo 64-bit) | Funziona per metriche base, warning su stack walk | 1 |
| Polling rate > sample rate (laptop sleep) | Salta sample, non interpola fittizio | 1 |
| Window minimized | Riduce render rate (no waste) | 1 |
| Window restored | Ripristina 60 fps | 1 |
| ETW session fails to start (permissions) | Continua polling-only + banner UI | 2 |
| ETW session lost (driver crash, …) | Tenta re-init, dopo 3 fail mostra banner | 2 |
| Symbol load fails | Mostra indirizzi raw, non blocca | 2 |
| GPU device lost (driver crash, RDP) | Re-init wgpu, log, continua | 1 |
| Sistema low memory | Ferma allocazioni nuove, mostra banner | 1 |
| File log non scrivibile | Disabilita logging file, continua | 1 |

## Test strategy

### Unit (veloci, headless)
- Logica pura: aggregation, delta calc, parsing
- Conversioni: FILETIME ↔ secondi, ETW GUID parsing
- Run on every commit

### Integration (con binario reale di test)
- Lanciamo `tests/fixtures/target_app.exe` (binario nostro) con comportamenti conosciuti:
  - Loop CPU-bound 5 s
  - Allocazione 100 MB poi free
  - Spawn 50 thread
  - Lock contention deliberata
- Verificare che Argus attaccato veda le metriche attese ± tolleranza

### Robustness
- Test in cui il target viene killato durante l'attach → no panic
- Test con PID random invalidi → errore controllato
- Test fuzz su input dell'utente (search box, ecc.)

### Performance
- Benchmark del target con/senza Argus attaccato → overhead < 1 %
- Profilare Argus stesso (RAM, CPU) in idle e active

## Stabilità long-running

Argus deve poter girare **almeno 8 ore continue** senza:

- Crescere in RAM (no leak in Argus stesso)
- Accumulare handle / GDI object / kernel resource
- Degradare in performance UI (no slowdown over time)
- Crescere il file log oltre il limite di rotazione

Test obbligatorio prima di ogni release fase: lasciare in esecuzione overnight con metric ricorrenti, snapshot risorse ogni ora.

## Graceful degradation

Se non possiamo fare X, **non** tentiamo di fare Y meglio per compensare. L'utente preferisce un dato mancante a un dato sbagliato.

Esempi:

| Situazione | Comportamento corretto | Comportamento SBAGLIATO |
|---|---|---|
| Non possiamo aprire process | Mostra error con suggerimento | Falsifica CPU = 0 |
| Symbol non trovato | Mostra `0x7FF8AC123456` | Inventa nome funzione |
| ETW non disponibile | Mostra solo polling + banner | Simula stack samples |
| GPU device lost | Re-init + log | Pannello vuoto in silenzio |

## Threat model (implicito)

Argus **non è uno strumento di sicurezza**. Assumiamo:

- L'utente è il sistema operativo
- Il processo target NON è ostile (no anti-debug bypass attempts)
- L'ambiente di esecuzione è fidato

Se queste assunzioni cadono, Argus può fallire — ma fallisce **safely** (no crash, no privilege escalation, no data corruption del target).
