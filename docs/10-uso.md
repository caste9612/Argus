# 10 – Come si usa Argus

Guida pratica all'uso. Per build/sviluppo vedi [`08-development.md`](08-development.md).

## Avvio

```powershell
cargo run --release            # build ottimizzata
# oppure l'eseguibile:           target\release\argus.exe
```

Argus apre una finestra con, a sinistra, la **lista processi** e, a destra, il
pannello con le **tab**: *Metriche*, *Flame graph*, *Timeline*, *Diff*.

### Utente vs amministratore — importante

| Esecuzione | Cosa funziona |
|---|---|
| **Utente** (normale) | Metriche polling (CPU/RAM/I-O/thread/handle), lista processi, record/diff/export. Flame graph e Timeline mostrano un banner "ETW non disponibile". |
| **Amministratore** | Tutto quanto sopra **+ flame graph e timeline** (richiedono ETW kernel, che ha bisogno dei privilegi). |

Per il profiling completo, avvia Argus **come amministratore** (tasto destro →
"Esegui come amministratore"). Senza, le altre funzioni restano comunque pienamente usabili.

## Collegarsi a un processo

1. Cerca il processo nella lista (casella *filtra per nome*). I tuoi processi
   (sessione utente) sono in cima; i servizi di sistema sotto, attenuati.
2. Ordina per **CPU**, **RAM** o **Nome**.
3. **Doppio click** sul processo (o selezionalo e premi **Collega**).
4. Per staccarti: **Scollega** nella barra in alto.

Da riga di comando puoi collegarti subito: `argus.exe --attach <PID>`
(opzionale `--tab flame|timeline|metriche|diff` per la tab iniziale).

## Le tab

- **Metriche** — KPI card + grafici time-series (CPU, working set/private,
  I/O lettura/scrittura, thread, handle). Ogni elemento ha un tooltip che spiega
  cosa significa. Con cattura ETW attiva (admin) appare anche una sezione
  **"Disco fisico (ETW)"**: byte letti/scritti, n° operazioni, dimensione media e
  **latenza per operazione** (mediana/p99/max in ms) per disco (attività di sistema
  durante la cattura), e una sezione **"Memoria
  (ETW)"** del target: hard page fault (page-in da disco), VirtualAlloc (memoria
  virtuale riservata) e saldo netto alloc−free (un saldo positivo e crescente
  segnala possibile crescita/leak).
- **Flame graph** — *dove* il processo spende tempo CPU (richiede admin).
  - **Click** su un frame per zoomare; **zoom out** torna alla radice.
  - **Cerca (regex)**: evidenzia le funzioni che combaciano, attenua le altre.
  - **Hover**: nome, numero di sample e percentuali (totale e self).
- **Timeline** — Gantt: stato di ogni thread del target nel tempo, dai context
  switch (richiede admin). Colori: **verde** = in esecuzione (Running), **ambra**
  = pronto ma in coda per la CPU (Ready), **blu** = in attesa (Waiting). In alto la
  riga **"Attese per causa"** riassume quanto tempo è speso in **Lock** (contesa di
  sincronizzazione), **I/O**, **Idle**; passa il mouse su un segmento per causa e
  durata. I thread più attivi in cima.
- **Diff** — confronto "prima/dopo" tra due sessioni salvate: grafici
  sovrapposti (A grigio = baseline, B blu = corrente) e le funzioni che cambiano
  di più (movers).

## Come interpretare i dati (diagnosi)

Argus serve a capire *perché* un programma si comporta così. Schemi tipici:

**È lento — dove va il tempo?**
- **CPU alta (vicina al 100% su uno o più core) + I/O bassa + poche attese** →
  *compute-bound*: il collo di bottiglia è il calcolo. Vai sul **Flame graph**: la
  barra più larga è la funzione che mangia CPU — ottimizza lì.
- **CPU bassa ma il programma è lento** → non sta calcolando, sta **aspettando**.
  Vai sulla **Timeline** e guarda la riga "Attese per causa":
  - tanto **Lock** → contesa di sincronizzazione (thread che si bloccano a
    vicenda) → riduci la sezione critica o ripensa il locking;
  - tanto **I/O** → aspetta disco/rete → il problema è l'I/O, non la CPU.

**Usa troppa memoria / perde memoria?**
- **Private bytes che crescono in modo monotono** (non scendono mai) → forte
  sospetto di **memory leak**. Un programma sano oscilla attorno a un valore.
- **Hard page fault alti** (sezione Memoria, ETW) → il working set non sta in RAM:
  il sistema pagina da disco → rallentamenti a scatti.
- **VirtualAlloc / saldo netto** in salita continua → riserve di memoria virtuale
  crescenti (possibile crescita o leak a livello di pagine).

**Perde risorse (handle)?**
- **Handle in crescita monotona** → file/socket/oggetti kernel non chiusi (handle
  leak). Un valore stabile e oscillante è sano.

**Il disco è il collo di bottiglia?**
- Sezione Disco (ETW): **latenza p99 molto più alta della mediana** → code di I/O
  occasionali (disco sotto pressione).

**Leggere il flame graph**
- Le barre sono larghe in proporzione al tempo CPU: cerca la funzione **larga e
  inaspettata**, spesso è lì l'ottimizzazione. **Self** = tempo nella funzione
  stessa; **totale** = lei più i suoi figli. La ricerca (regex) evidenzia un
  modulo/funzione.

**Provare un'ottimizzazione (Diff)**
- Salva una sessione *prima*, ottimizza, ricollega e **Confronta**: i *movers*
  mostrano quali funzioni sono cambiate di più — la prova che ha funzionato.

> Regola d'oro: prima **misura**, poi ottimizza. Argus serve proprio a misurare
> "con gli occhi".

## Registrare, riaprire, confrontare, esportare

Nella barra in alto (attivi quando c'è una sessione):

- **Salva** — salva la sessione corrente (metriche + flame) in un file
  `.argus` in `%LOCALAPPDATA%\Argus\captures`.
- **Apri** — riapre una sessione salvata in *replay* (la rivedi come live).
- **Esporta** — CSV (metriche), folded-stacks (apribile in
  [speedscope](https://www.speedscope.app/) / flamegraph.pl), SVG (flame graph
  statico, apribile nel browser), JSON (sessione completa: metadati + metriche +
  flame ad albero + attese/lock + memoria + disco, per analisi programmatica).
  Vanno in `%LOCALAPPDATA%\Argus\exports`. Le sessioni `.argus` salvate (v2)
  includono anch'esse disco/memoria/lock.
- **Confronta** — scegli una sessione `.argus` come baseline; il risultato
  appare nella tab **Diff**.

Workflow tipico "ottimizzazione": collega → *Salva* (prima) → ottimizza il codice
→ ricollega → *Confronta* con il file salvato → guarda i *movers* nella tab Diff.

## Dove finiscono i file

- Sessioni: `%LOCALAPPDATA%\Argus\captures\*.argus`
- Export: `%LOCALAPPDATA%\Argus\exports\*.{csv,folded.txt,svg}`
- Log: `%LOCALAPPDATA%\Argus\argus.log` (rotazione giornaliera)

## Note

- Se un processo è protetto (antivirus, PPL) o di sistema, l'attach può fallire
  con "accesso negato": serve l'esecuzione come amministratore (e alcuni restano
  comunque inaccessibili).
- I nomi delle funzioni nel flame graph si risolvono via DbgHelp dai moduli
  caricati: i moduli di sistema (ntdll, kernel32) e il **codice del target** con
  simboli disponibili mostrano il nome (es. `fixture!core::fmt::...`); dove il
  simbolo manca si vede `modulo!0xINDIRIZZO` (degrado graceful). Per nomi completi
  del tuo codice, compila con i `.pdb` accanto all'eseguibile.
