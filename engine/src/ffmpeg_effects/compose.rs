//! Several effects, one command: the filters of an entry's effects in the order they run, with the app's size cap between the two groups
//! (`Catalog::order`), written as one `-vf` chain, or - when an effect is a graph of its own (a blurred background, a picture laid over) - as
//! one `-filter_complex` graph ending in `[outv]`. There is no JavaScript twin: the pages show an entry's commands one after the other.

use super::engine::Built;

/// How the video is filtered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VideoFilter {
    /// Nothing to do.
    None,
    /// An ordinary chain on the one input (`-vf`).
    Simple(String),
    /// A filter graph over more than one input, ending in the label `[outv]` (`-filter_complex`, then `-map [outv]`).
    Complex(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Composed {
    /// Extra input files, in order: `[1:v]` is the first.
    pub inputs: Vec<String>,
    pub video: VideoFilter,
    /// The sound's chain (`-af`), when an effect has one (playing backwards).
    pub audio: Option<String>,
}

/// A graph being written: the finished parts, the stream the next one reads, and the plain filters waiting to be written as one part.
struct Graph {
    parts: Vec<String>,
    current: String,
    pending: Vec<String>,
    next: usize,
}

impl Graph {
    fn label(&mut self) -> String {
        let l = format!("v{}", self.next);
        self.next += 1;
        l
    }

    fn flush(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let out = self.label();
        self.parts.push(format!("[{}]{}[{out}]", self.current, self.pending.join(",")));
        self.current = out;
        self.pending.clear();
    }
}

fn bare(v: &Option<String>) -> Option<&str> {
    v.as_deref().filter(|s| !s.is_empty())
}

/// Write `before`, the size cap `cap` (a filter text, or none), then `after`, as one command.
pub fn compose(before: &[Built], cap: Option<&str>, after: &[Built]) -> Composed {
    let audio: Vec<&str> = before.iter().chain(after).filter_map(|b| bare(&b.audio)).collect();
    let audio = (!audio.is_empty()).then(|| audio.join(","));

    // the pieces in running order: Some(built) for an effect, None for the cap
    let order: Vec<Option<&Built>> = before.iter().map(Some).chain(std::iter::once(None)).chain(after.iter().map(Some)).collect();
    let plain = |b: &Built| b.chains.is_empty();
    let inputs_of = |inputs: &mut Vec<String>, b: &Built| inputs.extend(b.inputs.iter().cloned());

    if before.iter().chain(after).all(plain) {
        let mut inputs = Vec::new();
        let mut chain: Vec<String> = Vec::new();
        for piece in &order {
            match piece {
                Some(b) => {
                    inputs_of(&mut inputs, b);
                    chain.extend(bare(&b.video).map(str::to_owned));
                }
                None => chain.extend(cap.filter(|c| !c.is_empty()).map(str::to_owned)),
            }
        }
        let video = if chain.is_empty() { VideoFilter::None } else { VideoFilter::Simple(chain.join(",")) };
        return Composed { inputs, video, audio };
    }

    let mut g = Graph { parts: Vec::new(), current: "0:v".to_owned(), pending: Vec::new(), next: 0 };
    let mut inputs: Vec<String> = Vec::new();
    for (k, piece) in order.iter().enumerate() {
        let Some(b) = piece else {
            g.pending.extend(cap.filter(|c| !c.is_empty()).map(str::to_owned));
            continue;
        };
        if b.chains.is_empty() {
            inputs_of(&mut inputs, b);
            g.pending.extend(bare(&b.video).map(str::to_owned));
            continue;
        }
        g.flush();
        // this effect numbers its own extra inputs from 1; here they follow the ones already taken
        let first_input = inputs.len() + 1;
        inputs_of(&mut inputs, b);
        let start = g.current.clone();
        let end = g.label();
        let map = |l: &str| -> String {
            if l == "0:v" {
                return start.clone();
            }
            if l == "outv" {
                return end.clone();
            }
            match l.strip_suffix(":v").and_then(|n| n.parse::<usize>().ok()) {
                Some(n) if n >= 1 => format!("{}:v", first_input + n - 1),
                // a label of the effect's own: made apart from the other effects' by its place in the entry
                _ => format!("e{k}_{l}"),
            }
        };
        let label = |a: &[String]| a.iter().map(|l| format!("[{}]", map(l))).collect::<String>();
        for c in &b.chains {
            g.parts.push(format!("{}{}{}", label(&c.from), c.text, label(&c.to)));
        }
        g.current = end;
    }
    g.flush();
    let mut graph = g.parts.join(";");
    graph.truncate(graph.len() - g.current.len() - 2);
    graph.push_str("[outv]");
    Composed { inputs, video: VideoFilter::Complex(graph), audio }
}
