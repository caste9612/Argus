# 05 – UI Design

Principi visivi e di interazione. Quando aggiungiamo un pannello, widget, o grafico, va valutato contro questi standard.

## Filosofia

> *Show me everything important; hide everything else; explain what isn't obvious.*

L'utente apre Argus e in **5 secondi** capisce a colpo d'occhio:
- A quale processo è attaccato
- Se ci sono valori anomali (CPU alta, RAM crescente, errori)
- Dove guardare per approfondire

Solo dopo cerca i dettagli.

## Layout

Three-pane principale:

```
┌─────────────────────────────────────────────────────┐
│  TopBar   • stato sistema, attach, controlli        │
├──────────┬──────────────────────────────────────────┤
│          │                                          │
│ Process  │            Dashboard                     │
│  List    │   (cambia in base al tab attivo)         │
│          │                                          │
│          │   • Overview (KPI + line charts)         │
│          │   • Flame      (Fase 2)                  │
│          │   • Timeline   (Fase 2)                  │
│          │   • I/O detail (Fase 2)                  │
│          │                                          │
└──────────┴──────────────────────────────────────────┘
```

Process list resizable, collapsable. Mai > 30 % larghezza default.

## Linguaggio visivo

### Palette (dark, default)

Base scura per riposare gli occhi durante sessioni lunghe:

| Ruolo | Hex | Uso |
|---|---|---|
| Background | `#0E0E12` | Sfondo principale |
| Surface | `#16161D` | Pannelli, card |
| Surface-2 | `#1E1E28` | Componenti elevati |
| Border | `#2A2A38` | Separatori sottili |
| Text-primary | `#E8E8EE` | Testo normale |
| Text-secondary | `#9494A2` | Label, hint |
| Accent | `#7B61FF` | Selezione, focus |

### Semantica colore per le metriche

- **CPU**: gradiente verde→giallo→rosso (0→100 %)
- **Memoria (working set)**: blu (`#7CB9FF`)
- **Memoria (private)**: viola tenue (`#B49AFF`)
- **I/O Read**: verde acqua (`#7FE0B9`)
- **I/O Write**: ambra (`#F0C674`)
- **Thread / Handle**: viola tenue (`#B49AFF` / `#FFA5E0`)
- **Errore**: rosso (`#FF6B6B`)
- **Warning**: arancio (`#FFA94D`)
- **OK / verde**: (`#7CD992`)

Tutti i colori testati per colorblind safety (deuteranopia, protanopia).

### Tipografia

- **Sans-serif** per tutto: Inter (font asset) o fallback system UI
- **Monospace** per numeri, PID, paths: JetBrains Mono o Consolas
- Scale (px): 11 (caption) / 13 (body) / 15 (subhead) / 18 (h3) / 22 (h2) / 28 (h1)

### Spacing

Grid 4 px. Margini, padding, gap sono sempre multipli di 4.

## Componenti

### KPI Card

Big number, label sopra, color semantico. Larghezza fissa, altezza fissa, compatta.

```
┌──────────────┐
│ CPU          │
│  47.3 %      │
└──────────────┘
```

### Line chart

- Asse X = tempo (60 s sliding window default)
- Asse Y = unità della metrica
- Mai più di 2 serie per chart, se di più → split
- Hover mostra valore + timestamp esatto
- GPU-renderizzato (wgpu compute per smoothing opzionale)

### Flame graph (Fase 2)

- Verticale, root in alto, foglie in basso
- Larghezza = % tempo, colore = tipo (user / kernel / idle / antivirus)
- Hover mostra funzione + sample count + % padre + % totale
- Click zooma su un sottoalbero
- Search box sopra (regex, evidenzia match)

### Timeline (Fase 2)

- Una riga per thread
- Colore = stato (Running / Ready / Waiting / Blocked / I/O)
- Zoom orizzontale (timeline scrub)
- Tooltip su segmento: durata, stato, causa wait (lock GUID se disponibile)

## Interazione

### Stati che NON devono mai succedere

- ❌ Spinner infinito senza spiegazione
- ❌ Errori in dialog modale (usa toast/banner non bloccante)
- ❌ Bottoni che fanno cose diverse in base al contesto senza tooltip
- ❌ Numeri senza unità
- ❌ Tabelle che non si possono ordinare/filtrare se hanno > 10 righe

### Stati che DEVONO esserci

- ✅ "Not attached" con istruzione chiara (es. "Seleziona un processo dalla lista a sinistra")
- ✅ "Process exited" con bottone re-attach
- ✅ "Access denied" con suggerimento (run as admin)
- ✅ "Loading symbols" con progress bar (Fase 2)
- ✅ "ETW unavailable" con spiegazione del perché (Fase 2)

### Animazioni

- Transizioni morbide (200 ms ease-out) per cambio tab/pannello
- Mai animazioni > 400 ms (sembrano lente)
- Grafici si aggiornano fluidamente (interpolazione visiva tra sample, no salti)

## Accessibilità

- Tutto navigabile da tastiera (Tab, frecce, Enter, Esc)
- Contrast ratio AA su tutto il testo (4.5:1 minimo)
- Palette colorblind-safe (testata con simulatori)
- Tooltip su tutti i grafici (chi non vede il colore può leggere)
- Font scale al 110-125 % se l'utente ha DPI alto

## Cosa NON facciamo

- **No emoji nell'UI** (mai professionale)
- **No skeumorfismo** (no shadow ricchi, no gradient pesanti)
- **No animazioni gratuite** (ogni animazione ha una funzione)
- **No colori brillanti puri** (saturare-95 %, mai 100 %, faticano gli occhi)
