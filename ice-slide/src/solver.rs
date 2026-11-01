//! Exhaustive solver + difficulty metrics.
//!
//! A state is simply "the cell the player is resting on", so the full state
//! graph has at most `w*h` nodes and `4*w*h` edges. That makes a complete
//! analysis cheap enough to run thousands of times per second inside the
//! annealing loop.

use crate::level::{Dir, Level, Outcome, Tile};
use std::collections::VecDeque;

pub const UNREACHABLE: u32 = u32::MAX;

#[derive(Clone, Debug, Default)]
pub struct Analysis {
    pub solvable: bool,
    /// Minimum number of moves from the start to the goal.
    pub par: u32,
    /// One optimal solution.
    pub solution: Vec<Dir>,
    /// Number of distinct resting cells reachable from the start (incl. start,
    /// excl. the goal).
    pub reachable: u32,
    /// Reachable resting cells from which the goal can never be reached.
    /// Getting there means you're soft-locked.
    pub dead_ends: u32,
    /// Moves from "alive" cells that drop you into a hole.
    pub sink_moves: u32,
    /// All moves from "alive" cells that actually go somewhere.
    pub live_moves: u32,
    /// Number of distinct shortest solutions (saturating).
    pub optimal_paths: u64,
    /// Steps along the solution where the move that looks best (lands closest
    /// to the goal as the crow flies) is actually not optimal.
    pub misdirections: u32,
    /// Steps along the solution where more than one move is "productive"
    /// (doesn't kill you or soft-lock you) — i.e. real decisions.
    pub decisions: u32,
    /// For every cell: minimum moves to reach the goal (UNREACHABLE if none).
    pub to_goal: Vec<u32>,
    /// For every cell: whether it's reachable from the start.
    pub reached: Vec<bool>,
    /// For unsolvable maps: Manhattan distance from the goal to the closest
    /// reachable cell. Used to guide the annealer towards solvability.
    pub gap: u32,
    /// For unsolvable maps: how many cells could reach the goal at all.
    pub coreach: u32,
}

impl Analysis {
    pub fn dead_end_ratio(&self) -> f64 {
        if self.reachable == 0 {
            0.0
        } else {
            self.dead_ends as f64 / self.reachable as f64
        }
    }

    pub fn hazard_ratio(&self) -> f64 {
        if self.live_moves == 0 {
            0.0
        } else {
            self.sink_moves as f64 / self.live_moves as f64
        }
    }

    pub fn misdirection_ratio(&self) -> f64 {
        if self.par == 0 {
            0.0
        } else {
            self.misdirections as f64 / self.par as f64
        }
    }

    /// A single human-friendly 0..10 difficulty estimate.
    pub fn difficulty(&self) -> f64 {
        if !self.solvable {
            return 0.0;
        }
        let par = self.par as f64;
        // Calibrated so that maps generated with `Settings::from_difficulty(d)`
        // score roughly `d`.
        let length = (0.28 * (par - 2.0)).max(0.15);
        // Traps and misleading moves multiply the effort.
        let traps = 1.0
            + 1.2 * self.dead_end_ratio()
            + 0.8 * self.hazard_ratio()
            + 0.9 * self.misdirection_ratio()
            + 0.4 * (self.decisions as f64 / par.max(1.0));
        // A unique solution is harder to stumble upon than many.
        let uniq = 1.0 / (1.0 + 0.15 * (self.optimal_paths as f64).log2());
        (length * traps * (0.85 + 0.15 * uniq)).clamp(0.0, 10.0)
    }
}

fn manhattan(lv: &Level, a: usize, b: usize) -> u32 {
    let (ax, ay) = lv.xy(a);
    let (bx, by) = lv.xy(b);
    (ax.abs_diff(bx) + ay.abs_diff(by)) as u32
}

/// Cells from which the goal is reachable, considering every cell that isn't
/// a rock or hole as a potential resting spot.
fn coreachable(lv: &Level) -> Vec<bool> {
    let n = lv.len();
    let mut rev: Vec<Vec<usize>> = vec![Vec::new(); n];
    for c in 0..n {
        if c == lv.goal || matches!(lv.tiles[c], Tile::Rock | Tile::Hole) {
            continue;
        }
        for d in Dir::ALL {
            if let Outcome::Stopped(x) | Outcome::Won(x) = lv.slide(c, d)
                && x != c {
                    rev[x].push(c);
                }
        }
    }
    let mut seen = vec![false; n];
    seen[lv.goal] = true;
    let mut q = VecDeque::from([lv.goal]);
    while let Some(c) = q.pop_front() {
        for &p in &rev[c] {
            if !seen[p] {
                seen[p] = true;
                q.push_back(p);
            }
        }
    }
    seen
}

pub fn analyze(lv: &Level) -> Analysis {
    let n = lv.len();
    let mut a = Analysis {
        to_goal: vec![UNREACHABLE; n],
        reached: vec![false; n],
        ..Default::default()
    };

    // --- forward BFS from the start --------------------------------------
    let mut dist = vec![UNREACHABLE; n];
    let mut paths = vec![0u64; n];
    let mut parent = vec![(usize::MAX, Dir::Up); n];
    let mut q = VecDeque::new();
    dist[lv.start] = 0;
    paths[lv.start] = 1;
    a.reached[lv.start] = true;
    q.push_back(lv.start);
    // Cache transitions as we discover them.
    let mut trans: Vec<Option<Outcome>> = vec![None; n * 4];
    let start_is_goal = lv.start == lv.goal;
    while let Some(c) = q.pop_front() {
        if c == lv.goal {
            continue; // terminal
        }
        for (k, d) in Dir::ALL.iter().enumerate() {
            let out = lv.slide(c, *d);
            trans[c * 4 + k] = Some(out);
            let nxt = match out {
                Outcome::Stopped(x) | Outcome::Won(x) => x,
                Outcome::Sank(_) => continue,
            };
            if nxt == c {
                continue;
            }
            if dist[nxt] == UNREACHABLE {
                dist[nxt] = dist[c] + 1;
                paths[nxt] = paths[c];
                parent[nxt] = (c, *d);
                a.reached[nxt] = true;
                q.push_back(nxt);
            } else if dist[nxt] == dist[c] + 1 {
                paths[nxt] = paths[nxt].saturating_add(paths[c]);
            }
        }
    }

    a.reachable = a
        .reached
        .iter()
        .enumerate()
        .filter(|&(i, &r)| r && i != lv.goal)
        .count() as u32;

    a.solvable = !start_is_goal && dist[lv.goal] != UNREACHABLE;
    if !a.solvable {
        // Which cells *could* reach the goal (ignoring whether we can get
        // there)? The gap between that set and what we can reach gives the
        // annealer a gradient towards solvability.
        let coreach = coreachable(lv);
        a.coreach = coreach.iter().filter(|&&b| b).count() as u32;
        a.gap = (0..n)
            .filter(|&i| a.reached[i])
            .flat_map(|i| {
                let co = &coreach;
                (0..n).filter(move |&j| co[j]).map(move |j| manhattan(lv, i, j))
            })
            .min()
            .unwrap_or(u32::MAX);
        a.dead_ends = a.reachable;
        return a;
    }
    a.par = dist[lv.goal];
    a.optimal_paths = paths[lv.goal];
    let mut sol = Vec::with_capacity(a.par as usize);
    let mut c = lv.goal;
    while c != lv.start {
        let (p, d) = parent[c];
        sol.push(d);
        c = p;
    }
    sol.reverse();
    a.solution = sol;

    // --- reverse BFS from the goal over the reachable subgraph ----------
    let mut rev: Vec<Vec<usize>> = vec![Vec::new(); n];
    for c in 0..n {
        if !a.reached[c] || c == lv.goal {
            continue;
        }
        for k in 0..4 {
            if let Some(Outcome::Stopped(x) | Outcome::Won(x)) = trans[c * 4 + k]
                && x != c {
                    rev[x].push(c);
                }
        }
    }
    a.to_goal[lv.goal] = 0;
    q.clear();
    q.push_back(lv.goal);
    while let Some(c) = q.pop_front() {
        for &p in &rev[c] {
            if a.to_goal[p] == UNREACHABLE {
                a.to_goal[p] = a.to_goal[c] + 1;
                q.push_back(p);
            }
        }
    }

    // --- dead ends & hazards --------------------------------------------
    for c in 0..n {
        if !a.reached[c] || c == lv.goal {
            continue;
        }
        if a.to_goal[c] == UNREACHABLE {
            a.dead_ends += 1;
            continue;
        }
        for k in 0..4 {
            match trans[c * 4 + k] {
                Some(Outcome::Sank(_)) => {
                    a.sink_moves += 1;
                    a.live_moves += 1;
                }
                Some(Outcome::Stopped(x) | Outcome::Won(x)) if x != c => a.live_moves += 1,
                _ => {}
            }
        }
    }

    // --- misdirection & decisions along the optimal line -----------------
    let mut c = lv.start;
    for d in &a.solution {
        let here = a.to_goal[c];
        let mut best_look = u32::MAX;
        let mut tempting_is_optimal = false;
        let mut productive = 0;
        for k in 0..4 {
            let Some(Outcome::Stopped(x) | Outcome::Won(x)) = trans[c * 4 + k] else {
                continue;
            };
            if x == c {
                continue;
            }
            if a.to_goal[x] != UNREACHABLE {
                productive += 1;
            }
            let look = manhattan(lv, x, lv.goal);
            let optimal = a.to_goal[x] + 1 == here;
            if look < best_look {
                best_look = look;
                tempting_is_optimal = optimal;
            } else if look == best_look {
                // A tie with an optimal move isn't misleading.
                tempting_is_optimal |= optimal;
            }
        }
        if !tempting_is_optimal {
            a.misdirections += 1;
        }
        if productive > 1 {
            a.decisions += 1;
        }
        c = lv.slide(c, *d).cell();
    }

    a
}

/// The best move to make from `cell`, if the goal is reachable from there.
pub fn hint(lv: &Level, an: &Analysis, cell: usize) -> Option<Dir> {
    let here = *an.to_goal.get(cell)?;
    if here == UNREACHABLE || here == 0 {
        return None;
    }
    Dir::ALL.into_iter().find(|&d| match lv.slide(cell, d) {
        Outcome::Stopped(x) | Outcome::Won(x) => an.to_goal[x] + 1 == here,
        Outcome::Sank(_) => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::GoalRule;

    fn replay(lv: &Level, sol: &[Dir]) -> bool {
        let mut c = lv.start;
        for (i, d) in sol.iter().enumerate() {
            match lv.slide(c, *d) {
                Outcome::Won(_) => return i + 1 == sol.len(),
                Outcome::Stopped(x) => c = x,
                Outcome::Sank(_) => return false,
            }
        }
        false
    }

    #[test]
    fn solves_simple_map() {
        let lv = Level::parse(
            "
            S....
            ....#
            .....
            #...G
            ",
            GoalRule::MustStop,
        );
        let a = analyze(&lv);
        assert!(a.solvable);
        // Right -> (4,0); Down -> stops at (4,0)? rock at (4,1) so no move.
        // Down -> (0,2) (rock at (0,3)); Right -> (4,2); Down -> (4,3)=G.
        assert_eq!(a.par, 3);
        assert!(replay(&lv, &a.solution));
        assert_eq!(hint(&lv, &a, lv.start), Some(a.solution[0]));
    }

    #[test]
    fn detects_unsolvable() {
        let lv = Level::parse(
            "
            S.#.
            ..#G
            ",
            GoalRule::MustStop,
        );
        let a = analyze(&lv);
        assert!(!a.solvable);
        assert!(a.gap > 0);
    }

    /// Independent check: a reachable cell is a dead end iff a fresh BFS
    /// from it never reaches the goal.
    #[test]
    fn dead_ends_match_brute_force() {
        use crate::level::Tile;
        use crate::rng::Rng;
        let mut rng = Rng::new(7);
        let mut seen_dead = 0;
        for _ in 0..300 {
            let rule = if rng.chance(0.5) { GoalRule::MustStop } else { GoalRule::PassThrough };
            let mut lv = Level::new(7, 6, rule);
            for t in lv.tiles.iter_mut() {
                let r = rng.f64();
                *t = if r < 0.2 { Tile::Rock } else if r < 0.27 { Tile::Floor } else if r < 0.31 { Tile::Hole } else { Tile::Ice };
            }
            lv.start = rng.below(lv.len());
            lv.goal = rng.below(lv.len());
            lv.tiles[lv.start] = Tile::Ice;
            lv.tiles[lv.goal] = Tile::Ice;
            let a = analyze(&lv);
            if !a.solvable {
                continue;
            }
            assert!(replay(&lv, &a.solution));
            assert_eq!(a.solution.len() as u32, a.par);
            let mut dead = 0;
            for c in 0..lv.len() {
                if !a.reached[c] || c == lv.goal {
                    continue;
                }
                let mut probe = lv.clone();
                probe.start = c;
                let pa = analyze(&probe);
                let alive = pa.solvable;
                assert_eq!(alive, a.to_goal[c] != UNREACHABLE, "cell {c}\n{lv}");
                if alive {
                    assert_eq!(pa.par, a.to_goal[c]);
                } else {
                    dead += 1;
                }
            }
            assert_eq!(dead, a.dead_ends);
            seen_dead += dead;
        }
        assert!(seen_dead > 0, "test never exercised a dead end");
    }

    #[test]
    fn holes_count_as_hazards() {
        let lv = Level::parse(
            "
            S..O
            ....
            ...G
            ",
            GoalRule::MustStop,
        );
        let a = analyze(&lv);
        assert!(a.solvable);
        assert!(a.sink_moves >= 1);
        assert!(a.hazard_ratio() > 0.0);
    }

    #[test]
    fn pass_through_is_easier() {
        let map = "
            S...G.
            ......
        ";
        let stop = analyze(&Level::parse(map, GoalRule::MustStop));
        let pass = analyze(&Level::parse(map, GoalRule::PassThrough));
        assert_eq!(pass.par, 1);
        assert!(!stop.solvable || stop.par > 1);
    }
}
