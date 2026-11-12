//! Fuzz por mutação do validador de UI declarativa (entrada não confiável vinda de mini-apps).
//! Propriedade: nunca entra em pânico, e tudo que é aceito respeita os limites do catálogo.
use superapp_ui::{Event, Node, MAX_DEPTH, MAX_NODES, MAX_OUTPUT_BYTES};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

fn depth(n: &Node) -> usize {
    match n {
        Node::Stack { children } | Node::Row { children } => {
            1 + children.iter().map(depth).max().unwrap_or(0)
        }
        _ => 1,
    }
}

fn count(n: &Node) -> usize {
    match n {
        Node::Stack { children } | Node::Row { children } => {
            1 + children.iter().map(count).sum::<usize>()
        }
        _ => 1,
    }
}

#[test]
fn mutated_ui_trees_never_panic_and_accepted_ones_respect_limits() {
    let seeds = [
        r#"{"type":"stack","children":[{"type":"text","text":"oi"},{"type":"button","id":"inc","label":"+"},{"type":"list","items":["a","b","c"]}]}"#,
        r#"{"type":"text","text":"só texto com acentuação ção"}"#,
        r#"{"type":"stack","children":[{"type":"heading","text":"T"},{"type":"row","children":[{"type":"input","id":"a","label":"x","value":"v"},{"type":"button","id":"ok","label":"Ok"}]},{"type":"checkbox","id":"c","label":"y","checked":true},{"type":"select","id":"s","label":"z","options":["a","b"],"selected":"b"},{"type":"textarea","id":"t","label":"n","value":"a b","rows":3},{"type":"divider"},{"type":"progress","value":10}]}"#,
        r#"{"type":"action_list","items":[{"text":"a","done":true,"actions":[{"id":"x","label":"X"}]},{"text":"b"}]}"#,
        r#"{"type":"stack","children":[{"type":"stack","children":[{"type":"text","text":"x"}]}]}"#,
    ];
    let mut rng = Rng(0x1234_5678_9ABC_DEF1);
    let mut accepted = 0;
    for _ in 0..40_000 {
        let mut b = seeds[rng.below(seeds.len())].as_bytes().to_vec();
        for _ in 0..1 + rng.below(5) {
            let p = rng.below(b.len().max(1));
            match rng.below(4) {
                0 => {
                    let i = p.min(b.len() - 1);
                    b[i] = rng.next() as u8;
                }
                1 if b.len() > 1 => {
                    b.remove(p.min(b.len() - 1));
                }
                2 => b.insert(p.min(b.len()), rng.next() as u8),
                _ => {
                    // Duplica um trecho (provoca aninhamento e repetição).
                    let a = rng.below(b.len());
                    let e = (a + 1 + rng.below(30)).min(b.len());
                    let chunk = b[a..e].to_vec();
                    b.splice(p.min(b.len())..p.min(b.len()), chunk);
                }
            }
        }
        let s = String::from_utf8_lossy(&b).into_owned();
        if let Ok(n) = Node::parse(&s) {
            accepted += 1;
            assert!(depth(&n) <= MAX_DEPTH, "profundidade acima do limite: {s}");
            assert!(count(&n) <= MAX_NODES, "nós acima do limite");
            assert!(s.len() <= MAX_OUTPUT_BYTES);
        }
    }
    assert!(
        accepted > 0,
        "o fuzz precisa também exercitar o caminho aceito"
    );
}

#[test]
fn arbitrary_events_never_panic() {
    let mut rng = Rng(0xFEED_FACE_0BAD_F00D);
    for _ in 0..30_000 {
        let n = rng.below(80);
        let b: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
        let s = String::from_utf8_lossy(&b);
        if let Ok(e) = serde_json::from_str::<Event>(&s) {
            let _ = e.validate();
        }
    }
    // E ids arbitrários direto no validador.
    for _ in 0..30_000 {
        let n = rng.below(40);
        let id: String = (0..n)
            .map(|_| char::from_u32(rng.next() as u32 % 0x300).unwrap_or('x'))
            .collect();
        let _ = Event::click(id).validate();
    }
}
