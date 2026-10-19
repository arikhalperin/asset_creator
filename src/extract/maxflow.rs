//! Boykov–Kolmogorov max-flow (the "maxflow" algorithm used for graph cuts
//! in vision): search trees from both terminals are grown, augmented along,
//! and repaired, instead of being rebuilt for every path.

use std::collections::VecDeque;

const NONE: u32 = u32::MAX;
const TERMINAL: u32 = u32::MAX - 1;
const ORPHAN: u32 = u32::MAX - 2;

/// A flow network over `n` nodes with terminal capacities.
pub struct Graph {
    // Nodes.
    first: Vec<u32>,
    /// Arc to the parent (`NONE`, `TERMINAL`, `ORPHAN`, or an arc index).
    parent: Vec<u32>,
    is_sink: Vec<bool>,
    /// Residual terminal capacity: > 0 towards the source, < 0 towards the sink.
    tr_cap: Vec<f32>,
    ts: Vec<u32>,
    dist: Vec<u32>,
    active: Vec<bool>,
    // Arcs (stored in sister pairs 2k, 2k + 1).
    head: Vec<u32>,
    next: Vec<u32>,
    r_cap: Vec<f32>,
}

impl Graph {
    pub fn new(nodes: usize, arcs_hint: usize) -> Self {
        Self {
            first: vec![NONE; nodes],
            parent: vec![NONE; nodes],
            is_sink: vec![false; nodes],
            tr_cap: vec![0.0; nodes],
            ts: vec![0; nodes],
            dist: vec![0; nodes],
            active: vec![false; nodes],
            head: Vec::with_capacity(arcs_hint),
            next: Vec::with_capacity(arcs_hint),
            r_cap: Vec::with_capacity(arcs_hint),
        }
    }

    /// An undirected edge (capacity `cap` both ways).
    pub fn edge(&mut self, a: usize, b: usize, cap: f32) {
        for (from, to) in [(a, b), (b, a)] {
            self.head.push(to as u32);
            self.r_cap.push(cap);
            self.next.push(self.first[from]);
            self.first[from] = self.head.len() as u32 - 1;
        }
    }

    /// Terminal capacities: `to_source` is paid when the node ends on the
    /// sink side, `to_sink` when it ends on the source side.
    pub fn terminals(&mut self, node: usize, to_source: f32, to_sink: f32) {
        self.tr_cap[node] += to_source - to_sink;
    }

    fn sister(a: u32) -> u32 {
        a ^ 1
    }

    fn set_active(&mut self, queue: &mut VecDeque<u32>, i: u32) {
        if !self.active[i as usize] {
            self.active[i as usize] = true;
            queue.push_back(i);
        }
    }

    fn next_active(&mut self, queue: &mut VecDeque<u32>) -> Option<u32> {
        while let Some(i) = queue.pop_front() {
            self.active[i as usize] = false;
            if self.parent[i as usize] != NONE {
                return Some(i);
            }
        }
        None
    }

    fn augment(&mut self, middle: u32, orphans: &mut VecDeque<u32>) {
        let sis = Self::sister(middle);
        // Bottleneck.
        let mut b = self.r_cap[middle as usize];
        let mut i = self.head[sis as usize];
        loop {
            let a = self.parent[i as usize];
            if a == TERMINAL {
                break;
            }
            b = b.min(self.r_cap[Self::sister(a) as usize]);
            i = self.head[a as usize];
        }
        b = b.min(self.tr_cap[i as usize]);
        let mut i = self.head[middle as usize];
        loop {
            let a = self.parent[i as usize];
            if a == TERMINAL {
                break;
            }
            b = b.min(self.r_cap[a as usize]);
            i = self.head[a as usize];
        }
        b = b.min(-self.tr_cap[i as usize]);
        // Push.
        self.r_cap[sis as usize] += b;
        self.r_cap[middle as usize] -= b;
        let mut i = self.head[sis as usize];
        loop {
            let a = self.parent[i as usize];
            if a == TERMINAL {
                break;
            }
            self.r_cap[a as usize] += b;
            let s = Self::sister(a) as usize;
            self.r_cap[s] -= b;
            if self.r_cap[s] <= 0.0 {
                self.parent[i as usize] = ORPHAN;
                orphans.push_front(i);
            }
            i = self.head[a as usize];
        }
        self.tr_cap[i as usize] -= b;
        if self.tr_cap[i as usize] <= 0.0 {
            self.parent[i as usize] = ORPHAN;
            orphans.push_front(i);
        }
        let mut i = self.head[middle as usize];
        loop {
            let a = self.parent[i as usize];
            if a == TERMINAL {
                break;
            }
            self.r_cap[Self::sister(a) as usize] += b;
            self.r_cap[a as usize] -= b;
            if self.r_cap[a as usize] <= 0.0 {
                self.parent[i as usize] = ORPHAN;
                orphans.push_front(i);
            }
            i = self.head[a as usize];
        }
        self.tr_cap[i as usize] += b;
        if self.tr_cap[i as usize] >= 0.0 {
            self.parent[i as usize] = ORPHAN;
            orphans.push_front(i);
        }
    }

    /// Re-attaches orphan `i` to its tree, or frees it.
    fn adopt(&mut self, i: u32, time: u32, queue: &mut VecDeque<u32>, orphans: &mut VecDeque<u32>) {
        let sink = self.is_sink[i as usize];
        let mut best: Option<(u32, u32)> = None; // (arc, distance)
        let mut a0 = self.first[i as usize];
        while a0 != NONE {
            // The arc from the candidate parent j to i must have capacity
            // (towards the sink tree: from i to j).
            let usable = if sink {
                self.r_cap[a0 as usize] > 0.0
            } else {
                self.r_cap[Self::sister(a0) as usize] > 0.0
            };
            let j0 = self.head[a0 as usize];
            if usable && self.is_sink[j0 as usize] == sink && self.parent[j0 as usize] != NONE {
                // Does j0 trace back to the terminal?
                let mut j = j0;
                let mut d = 0u32;
                let origin = loop {
                    if self.ts[j as usize] == time {
                        d += self.dist[j as usize];
                        break true;
                    }
                    let a = self.parent[j as usize];
                    d += 1;
                    if a == TERMINAL {
                        self.ts[j as usize] = time;
                        self.dist[j as usize] = 1;
                        break true;
                    }
                    if a == ORPHAN {
                        break false;
                    }
                    j = self.head[a as usize];
                };
                if origin {
                    if best.is_none_or(|(_, bd)| d < bd) {
                        best = Some((a0, d));
                    }
                    let mut j = j0;
                    let mut d = d;
                    while self.ts[j as usize] != time {
                        self.ts[j as usize] = time;
                        self.dist[j as usize] = d;
                        d = d.saturating_sub(1);
                        j = self.head[self.parent[j as usize] as usize];
                    }
                }
            }
            a0 = self.next[a0 as usize];
        }
        if let Some((a, d)) = best {
            self.parent[i as usize] = a;
            self.ts[i as usize] = time;
            self.dist[i as usize] = d + 1;
            return;
        }
        // No parent: i becomes free.
        self.parent[i as usize] = NONE;
        let mut a0 = self.first[i as usize];
        while a0 != NONE {
            let j = self.head[a0 as usize];
            let a = self.parent[j as usize];
            if self.is_sink[j as usize] == sink && a != NONE {
                let residual = if sink {
                    self.r_cap[a0 as usize] > 0.0
                } else {
                    self.r_cap[Self::sister(a0) as usize] > 0.0
                };
                if residual {
                    self.set_active(queue, j);
                }
                if a != TERMINAL && a != ORPHAN && self.head[a as usize] == i {
                    self.parent[j as usize] = ORPHAN;
                    orphans.push_back(j);
                }
            }
            a0 = self.next[a0 as usize];
        }
    }

    /// Runs max-flow; returns, per node, whether it is on the source side.
    pub fn min_cut(&mut self) -> Vec<bool> {
        let n = self.first.len();
        let mut queue = VecDeque::new();
        let mut orphans = VecDeque::new();
        for i in 0..n {
            if self.tr_cap[i] > 0.0 {
                self.is_sink[i] = false;
                self.parent[i] = TERMINAL;
                self.dist[i] = 1;
                self.set_active(&mut queue, i as u32);
            } else if self.tr_cap[i] < 0.0 {
                self.is_sink[i] = true;
                self.parent[i] = TERMINAL;
                self.dist[i] = 1;
                self.set_active(&mut queue, i as u32);
            }
        }
        let mut time = 0u32;
        let mut current: Option<u32> = None;
        loop {
            let i = match current.filter(|&i| self.parent[i as usize] != NONE) {
                Some(i) => i,
                None => match self.next_active(&mut queue) {
                    Some(i) => i,
                    None => break,
                },
            };
            // Grow the tree of i.
            let sink = self.is_sink[i as usize];
            let mut found = None;
            let mut a = self.first[i as usize];
            while a != NONE {
                let cap = if sink {
                    self.r_cap[Self::sister(a) as usize]
                } else {
                    self.r_cap[a as usize]
                };
                if cap > 0.0 {
                    let j = self.head[a as usize] as usize;
                    if self.parent[j] == NONE {
                        self.is_sink[j] = sink;
                        self.parent[j] = Self::sister(a);
                        self.ts[j] = self.ts[i as usize];
                        self.dist[j] = self.dist[i as usize] + 1;
                        self.set_active(&mut queue, j as u32);
                    } else if self.is_sink[j] != sink {
                        // Middle arc, oriented source tree → sink tree.
                        found = Some(if sink { Self::sister(a) } else { a });
                        break;
                    } else if self.ts[j] <= self.ts[i as usize]
                        && self.dist[j] > self.dist[i as usize]
                    {
                        self.parent[j] = Self::sister(a);
                        self.ts[j] = self.ts[i as usize];
                        self.dist[j] = self.dist[i as usize] + 1;
                    }
                }
                a = self.next[a as usize];
            }
            time += 1;
            match found {
                Some(middle) => {
                    current = Some(i);
                    self.augment(middle, &mut orphans);
                    while let Some(o) = orphans.pop_front() {
                        self.adopt(o, time, &mut queue, &mut orphans);
                    }
                }
                None => current = None,
            }
        }
        (0..n)
            .map(|i| self.parent[i] != NONE && !self.is_sink[i])
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_cut() {
        // 0 — 1 — 2 in a line; 0 prefers source, 2 prefers sink, weak link 1–2.
        let mut g = Graph::new(3, 16);
        g.terminals(0, 10.0, 0.0);
        g.terminals(2, 0.0, 10.0);
        g.edge(0, 1, 5.0);
        g.edge(1, 2, 1.0);
        assert_eq!(g.min_cut(), vec![true, true, false]);
    }

    /// Brute force over all labelings of a small random graph.
    #[test]
    fn matches_brute_force_min_cut() {
        let mut seed = 12345u64;
        let mut rand = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) % 100) as f32
        };
        for _ in 0..30 {
            let n = 8;
            let terms: Vec<(f32, f32)> = (0..n).map(|_| (rand(), rand())).collect();
            let mut edges = Vec::new();
            for a in 0..n {
                for b in a + 1..n {
                    if rand() < 40.0 {
                        edges.push((a, b, rand() / 2.0));
                    }
                }
            }
            let mut g = Graph::new(n, 64);
            for (i, &(s, t)) in terms.iter().enumerate() {
                g.terminals(i, s, t);
            }
            for &(a, b, c) in &edges {
                g.edge(a, b, c);
            }
            let side = g.min_cut();
            let cost = |side: &[bool]| -> f32 {
                let mut c = 0.0;
                for (i, &(s, t)) in terms.iter().enumerate() {
                    c += if side[i] { t } else { s };
                }
                for &(a, b, w) in &edges {
                    if side[a] != side[b] {
                        c += w;
                    }
                }
                c
            };
            let best = (0..1u32 << n)
                .map(|m| cost(&(0..n).map(|i| m >> i & 1 == 1).collect::<Vec<_>>()))
                .fold(f32::INFINITY, f32::min);
            assert!(
                (cost(&side) - best).abs() < 1e-3,
                "{} vs {best}",
                cost(&side)
            );
        }
    }
}
