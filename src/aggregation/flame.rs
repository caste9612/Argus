//! Flame graph: aggregazione degli stack sample in un albero pesato.
//!
//! Ogni stack campionato — sequenza di frame dal *root* (la funzione più esterna,
//! es. l'entry point del thread) alla *foglia* (la funzione in cui la CPU è stata
//! sorpresa) — viene fuso in un albero: nodi identici lungo lo stesso percorso
//! condividono lo stesso nodo, con un contatore. La larghezza di un nodo nel
//! disegno è proporzionale ai sample che lo attraversano (`total`): più è largo,
//! più tempo CPU vi è passato. È *la* visualizzazione di un profiler
//! (vedi docs/04-metrics.md).
//!
//! La struttura è **pura**: riceve nomi di frame già risolti dal layer simboli e
//! li aggrega, senza toccare alcuna API Win32. È quindi interamente testabile in
//! isolamento, senza ETW né privilegi.

use std::collections::HashMap;

/// Identificatore di un nodo nell'arena. 0 è sempre la radice sintetica.
pub type NodeId = u32;

/// La radice sintetica: rappresenta "tutti i sample".
pub const ROOT: NodeId = 0;

/// Un nodo dell'albero: una funzione vista in una specifica posizione di stack.
#[derive(Clone)]
struct Node {
    /// Indice del nome nella tabella di interning.
    name: u32,
    parent: NodeId,
    /// Profondità nello stack (la radice è 0).
    depth: u16,
    /// Sample che attraversano questo nodo (== larghezza nel disegno).
    total: u64,
    /// Sample la cui foglia è esattamente questo nodo (self time).
    own: u64,
    /// Figli, in ordine di prima comparsa: stabile tra un update e l'altro, così
    /// i frame non "saltano" lateralmente mentre i contatori crescono.
    children: Vec<NodeId>,
}

/// Rettangolo di layout per il rendering: posizione orizzontale normalizzata
/// in `[0,1]` rispetto al nodo focus, alla profondità `depth`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub node: NodeId,
    pub depth: u16,
    pub x0: f64,
    pub x1: f64,
}

/// Albero flame graph con interning dei nomi dei frame.
#[derive(Clone)]
pub struct FlameGraph {
    nodes: Vec<Node>,
    /// `(genitore, name_id) -> figlio`. Una sola mappa per tutto l'albero: più
    /// economica di una HashMap per nodo.
    edges: HashMap<(NodeId, u32), NodeId>,
    names: Vec<String>,
    name_ids: HashMap<String, u32>,
    total_samples: u64,
}

impl Default for FlameGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl FlameGraph {
    pub fn new() -> Self {
        let mut g = Self {
            nodes: Vec::new(),
            edges: HashMap::new(),
            names: Vec::new(),
            name_ids: HashMap::new(),
            total_samples: 0,
        };
        g.reset_root();
        g
    }

    /// Azzera l'albero mantenendo la capacità allocata (riuso tra sessioni).
    pub fn clear(&mut self) {
        self.nodes.clear();
        self.edges.clear();
        self.names.clear();
        self.name_ids.clear();
        self.total_samples = 0;
        self.reset_root();
    }

    /// (Re)inizializza la radice e l'etichetta name_id 0.
    fn reset_root(&mut self) {
        // name_id 0 = etichetta della radice.
        let _ = self.intern("[tutto]");
        self.nodes.push(Node {
            name: 0,
            parent: ROOT,
            depth: 0,
            total: 0,
            own: 0,
            children: Vec::new(),
        });
    }

    /// Numero totale di stack aggregati.
    pub fn total_samples(&self) -> u64 {
        self.total_samples
    }

    /// Numero di nodi (radice inclusa).
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.total_samples == 0
    }

    /// Ricostruisce un albero da una lista piatta di nodi (per il caricamento di
    /// una sessione salvata, vedi `util`/`persist`). Ogni tupla è
    /// `(nome, genitore, profondità, total, own)`; il nodo 0 è la radice. I
    /// riferimenti al genitore non validi vengono ignorati (no panic su file
    /// corrotto). `total_samples` è passato a parte (== total della radice).
    pub fn from_nodes(nodes: &[(String, NodeId, u16, u64, u64)], total_samples: u64) -> Self {
        let mut g = Self {
            nodes: Vec::with_capacity(nodes.len().max(1)),
            edges: HashMap::new(),
            names: Vec::new(),
            name_ids: HashMap::new(),
            total_samples: 0,
        };
        if nodes.is_empty() {
            g.reset_root();
            return g;
        }
        for (i, (name, parent, depth, total, own)) in nodes.iter().enumerate() {
            let name_id = g.intern(name);
            g.nodes.push(Node {
                name: name_id,
                parent: *parent,
                depth: *depth,
                total: *total,
                own: *own,
                children: Vec::new(),
            });
            // Il nodo 0 è la radice (nessun genitore). Per gli altri, collega solo
            // se il genitore è già stato creato (indice valido, < i): negli alberi
            // ben formati il genitore precede sempre il figlio.
            if i > 0 {
                let p = *parent as usize;
                if p < i {
                    g.nodes[p].children.push(i as NodeId);
                    g.edges.insert((*parent, name_id), i as NodeId);
                }
            }
        }
        g.total_samples = total_samples;
        g
    }

    /// Aggiunge uno stack, dal frame più esterno (root) a quello più interno
    /// (foglia, dove la CPU è stata campionata). I nomi vengono internati e i
    /// nodi condivisi lungo i prefissi comuni.
    pub fn add_stack<S: AsRef<str>>(&mut self, frames: &[S]) {
        self.total_samples += 1;
        self.nodes[ROOT as usize].total += 1;

        let mut cur = ROOT;
        for f in frames {
            let nid = self.intern(f.as_ref());
            let child = match self.edges.get(&(cur, nid)) {
                Some(&c) => c,
                None => self.add_child(cur, nid),
            };
            self.nodes[child as usize].total += 1;
            cur = child;
        }
        // La foglia accumula il self time; per uno stack vuoto è la radice stessa.
        self.nodes[cur as usize].own += 1;
    }

    fn add_child(&mut self, parent: NodeId, name: u32) -> NodeId {
        let depth = self.nodes[parent as usize].depth.saturating_add(1);
        let id = self.nodes.len() as NodeId;
        self.nodes.push(Node {
            name,
            parent,
            depth,
            total: 0,
            own: 0,
            children: Vec::new(),
        });
        self.nodes[parent as usize].children.push(id);
        self.edges.insert((parent, name), id);
        id
    }

    fn intern(&mut self, name: &str) -> u32 {
        if let Some(&id) = self.name_ids.get(name) {
            return id;
        }
        let id = self.names.len() as u32;
        self.names.push(name.to_string());
        self.name_ids.insert(name.to_string(), id);
        id
    }

    // --- Accessor di sola lettura per il renderer ---

    /// Nome del frame di un nodo (stringa vuota se l'id non è valido).
    pub fn name_of(&self, node: NodeId) -> &str {
        let nid = self.nodes.get(node as usize).map_or(0, |n| n.name);
        self.names.get(nid as usize).map_or("", |s| s.as_str())
    }

    pub fn total_of(&self, node: NodeId) -> u64 {
        self.nodes.get(node as usize).map_or(0, |n| n.total)
    }

    pub fn own_of(&self, node: NodeId) -> u64 {
        self.nodes.get(node as usize).map_or(0, |n| n.own)
    }

    pub fn depth_of(&self, node: NodeId) -> u16 {
        self.nodes.get(node as usize).map_or(0, |n| n.depth)
    }

    pub fn parent_of(&self, node: NodeId) -> NodeId {
        self.nodes.get(node as usize).map_or(ROOT, |n| n.parent)
    }

    pub fn children_of(&self, node: NodeId) -> &[NodeId] {
        self.nodes
            .get(node as usize)
            .map_or(&[][..], |n| n.children.as_slice())
    }

    /// Calcola il layout per il rendering espandendo `focus` a piena larghezza.
    ///
    /// Restituisce, nell'ordine: la catena di antenati `radice..focus` (ciascuno
    /// a piena larghezza, così cliccandoli si torna indietro) e poi il
    /// sottoalbero del focus, con larghezze proporzionali ai `total`. Se `focus`
    /// non esiste, ritorna vuoto (nessun panic).
    pub fn layout(&self, focus: NodeId) -> Vec<Rect> {
        let mut out = Vec::with_capacity(self.nodes.len());
        if self.nodes.get(focus as usize).is_none() {
            return out;
        }

        // Antenati: risalgo dal focus alla radice, poi emetto dall'alto in basso.
        let mut chain = Vec::new();
        let mut a = focus;
        loop {
            chain.push(a);
            if a == ROOT {
                break;
            }
            a = self.nodes[a as usize].parent;
        }
        for &n in chain.iter().rev() {
            out.push(Rect {
                node: n,
                depth: self.nodes[n as usize].depth,
                x0: 0.0,
                x1: 1.0,
            });
        }

        // Discendenti del focus, con stack di lavoro esplicito invece della
        // ricorsione: robusto anche per stack patologicamente profondi.
        let mut work = vec![(focus, 0.0f64, 1.0f64)];
        while let Some((node, x0, x1)) = work.pop() {
            let total = self.nodes[node as usize].total as f64;
            if total <= 0.0 {
                continue;
            }
            let width = x1 - x0;
            let mut cursor = x0;
            for &c in &self.nodes[node as usize].children {
                let ct = self.nodes[c as usize].total as f64;
                let cw = width * (ct / total);
                let cx0 = cursor;
                let cx1 = cursor + cw;
                out.push(Rect {
                    node: c,
                    depth: self.nodes[c as usize].depth,
                    x0: cx0,
                    x1: cx1,
                });
                work.push((c, cx0, cx1));
                cursor = cx1;
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trova un nodo seguendo un percorso di nomi dalla radice (per i test).
    fn find(g: &FlameGraph, path: &[&str]) -> Option<NodeId> {
        let mut cur = ROOT;
        'outer: for &want in path {
            for &c in g.children_of(cur) {
                if g.name_of(c) == want {
                    cur = c;
                    continue 'outer;
                }
            }
            return None;
        }
        Some(cur)
    }

    fn sample_graph() -> FlameGraph {
        let mut g = FlameGraph::new();
        g.add_stack(&["main", "a", "b"]);
        g.add_stack(&["main", "a", "b"]);
        g.add_stack(&["main", "a", "c"]);
        g.add_stack(&["main", "d"]);
        g
    }

    #[test]
    fn merges_shared_prefixes_and_counts() {
        let g = sample_graph();
        assert_eq!(g.total_samples(), 4);
        assert_eq!(g.total_of(ROOT), 4);
        // root + main + a + b + c + d
        assert_eq!(g.node_count(), 6);

        let main = find(&g, &["main"]).unwrap();
        let a = find(&g, &["main", "a"]).unwrap();
        let b = find(&g, &["main", "a", "b"]).unwrap();
        let c = find(&g, &["main", "a", "c"]).unwrap();
        let d = find(&g, &["main", "d"]).unwrap();

        assert_eq!(g.total_of(main), 4);
        assert_eq!(g.own_of(main), 0);
        assert_eq!(g.total_of(a), 3);
        assert_eq!(g.total_of(b), 2);
        assert_eq!(g.own_of(b), 2);
        assert_eq!(g.total_of(c), 1);
        assert_eq!(g.own_of(c), 1);
        assert_eq!(g.total_of(d), 1);
        assert_eq!(g.own_of(d), 1);
        assert_eq!(g.depth_of(b), 3);
        assert_eq!(g.parent_of(b), a);
    }

    #[test]
    fn layout_widths_are_proportional() {
        let g = sample_graph();
        let rects: HashMap<NodeId, Rect> =
            g.layout(ROOT).into_iter().map(|r| (r.node, r)).collect();

        let main = find(&g, &["main"]).unwrap();
        let a = find(&g, &["main", "a"]).unwrap();
        let b = find(&g, &["main", "a", "b"]).unwrap();
        let c = find(&g, &["main", "a", "c"]).unwrap();
        let d = find(&g, &["main", "d"]).unwrap();

        let approx = |x: f64, y: f64| (x - y).abs() < 1e-9;

        // root e main coprono tutta la larghezza.
        assert!(approx(rects[&ROOT].x0, 0.0) && approx(rects[&ROOT].x1, 1.0));
        assert!(approx(rects[&main].x0, 0.0) && approx(rects[&main].x1, 1.0));
        // a = 3/4, d = 1/4 dei sample di main, affiancati.
        assert!(approx(rects[&a].x0, 0.0) && approx(rects[&a].x1, 0.75));
        assert!(approx(rects[&d].x0, 0.75) && approx(rects[&d].x1, 1.0));
        // b = 2/3, c = 1/3 della larghezza di a (0.75).
        assert!(approx(rects[&b].x0, 0.0) && approx(rects[&b].x1, 0.5));
        assert!(approx(rects[&c].x0, 0.5) && approx(rects[&c].x1, 0.75));
        // profondità corrette.
        assert_eq!(rects[&b].depth, 3);
    }

    #[test]
    fn focus_expands_subtree_and_shows_ancestors() {
        let g = sample_graph();
        let a = find(&g, &["main", "a"]).unwrap();
        let b = find(&g, &["main", "a", "b"]).unwrap();
        let rects: HashMap<NodeId, Rect> = g.layout(a).into_iter().map(|r| (r.node, r)).collect();

        let approx = |x: f64, y: f64| (x - y).abs() < 1e-9;
        // Gli antenati (root, main, a) sono a piena larghezza.
        assert!(approx(rects[&a].x0, 0.0) && approx(rects[&a].x1, 1.0));
        // b ora occupa 2/3 della larghezza piena (era 0.5 nella vista globale).
        assert!(approx(rects[&b].x0, 0.0) && approx(rects[&b].x1, 2.0 / 3.0));
    }

    #[test]
    fn clear_resets_but_keeps_root() {
        let mut g = sample_graph();
        g.clear();
        assert_eq!(g.total_samples(), 0);
        assert_eq!(g.node_count(), 1);
        assert!(g.is_empty());
        assert_eq!(g.total_of(ROOT), 0);
    }

    #[test]
    fn empty_stack_counts_as_root_self_time() {
        let mut g = FlameGraph::new();
        let empty: [&str; 0] = [];
        g.add_stack(&empty);
        assert_eq!(g.total_samples(), 1);
        assert_eq!(g.own_of(ROOT), 1);
        assert_eq!(g.node_count(), 1);
    }

    #[test]
    fn accepts_owned_strings_too() {
        let mut g = FlameGraph::new();
        let frames = vec![String::from("x"), String::from("y")];
        g.add_stack(&frames);
        assert_eq!(g.total_of(find(&g, &["x", "y"]).unwrap()), 1);
    }

    #[test]
    fn from_nodes_rebuilds_equivalent_tree() {
        let g = sample_graph();
        // Esporta in lista piatta, come fa `persist` tramite gli accessor.
        let nodes: Vec<(String, NodeId, u16, u64, u64)> = (0..g.node_count() as u32)
            .map(|i| {
                (
                    g.name_of(i).to_string(),
                    g.parent_of(i),
                    g.depth_of(i),
                    g.total_of(i),
                    g.own_of(i),
                )
            })
            .collect();
        let g2 = FlameGraph::from_nodes(&nodes, g.total_samples());

        assert_eq!(g2.total_samples(), g.total_samples());
        assert_eq!(g2.node_count(), g.node_count());
        let b = find(&g2, &["main", "a", "b"]).expect("nodo ricostruito");
        assert_eq!(g2.total_of(b), 2);
        assert_eq!(g2.own_of(b), 2);
        assert_eq!(g.layout(ROOT).len(), g2.layout(ROOT).len());
    }
}
