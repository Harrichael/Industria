//! Simulated-annealing level generator.
//!
//! We start from a random map and repeatedly *mutate* it (add/remove/move a
//! rock, move the start or goal, sprinkle snow or holes …). After every
//! mutation the solver measures the new map and a cost function tells us how
//! far its metrics are from the requested *difficulty zone*. Better maps are
//! always kept; worse ones are kept with probability `exp(-Δ/T)` where the
//! temperature `T` cools over time — classic simulated annealing. As soon as a
//! map lands inside every zone we stop.

use crate::level::{GoalRule, Level, Tile};
use crate::rng::Rng;
use crate::solver::{analyze, Analysis};

/// A target band `[center - tol, center + tol]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zone {
    pub center: f64,
    pub tol: f64,
}

impl Zone {
    pub const fn new(center: f64, tol: f64) -> Self {
        Zone { center, tol }
    }
    pub fn lo(&self) -> f64 {
        (self.center - self.tol).max(0.0)
    }
    pub fn hi(&self) -> f64 {
        self.center + self.tol
    }
    pub fn contains(&self, x: f64) -> bool {
        x >= self.lo() - 1e-9 && x <= self.hi() + 1e-9
    }
    /// 0 inside the zone, growing quadratically (in units of `scale`) outside.
    pub fn err(&self, x: f64, scale: f64) -> f64 {
        let d = if x < self.lo() {
            self.lo() - x
        } else if x > self.hi() {
            x - self.hi()
        } else {
            0.0
        };
        let e = d / scale;
        e * e + e // linear term keeps a gradient when close to the zone
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub w: usize,
    pub h: usize,
    pub rule: GoalRule,
    /// Minimum number of moves.
    pub par: Zone,
    /// Fraction of reachable resting spots that soft-lock you.
    pub dead_ends: Zone,
    /// Fraction of solution steps where the "obvious" move is wrong.
    pub misdirection: Zone,
    /// Fraction of live moves that drop you into a hole.
    pub hazard: Zone,
    /// Target fraction of the map covered by rocks / snow / holes.
    pub rock_density: f64,
    pub floor_density: f64,
    pub hole_density: f64,
    /// Annealing budget.
    pub max_iters: u32,
}

impl Settings {
    /// Sensible settings for an overall difficulty in `1.0..=10.0`.
    pub fn from_difficulty(d: f64) -> Settings {
        let d = d.clamp(1.0, 10.0);
        let t = (d - 1.0) / 9.0; // 0..1
        let w = (8.0 + t * 8.0).round() as usize;
        let h = (7.0 + t * 6.0).round() as usize;
        Settings {
            w,
            h,
            rule: if d < 3.5 {
                GoalRule::PassThrough
            } else {
                GoalRule::MustStop
            },
            par: Zone::new((3.0 + 1.7 * d).round(), (1.0 + 0.15 * d).round()),
            dead_ends: Zone::new(0.04 * d, 0.06),
            misdirection: Zone::new(0.045 * d, 0.12),
            hazard: if d < 3.0 {
                Zone::new(0.0, 0.0)
            } else {
                Zone::new(0.03 * (d - 2.0), 0.05)
            },
            rock_density: 0.16 + 0.006 * d,
            floor_density: 0.03,
            hole_density: if d < 3.0 { 0.0 } else { 0.006 * (d - 2.0) },
            max_iters: 60_000,
        }
    }

    pub fn holes_enabled(&self) -> bool {
        self.hole_density > 0.0
    }

    pub fn floors_enabled(&self) -> bool {
        self.floor_density > 0.0
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings::from_difficulty(5.0)
    }
}

/// Breakdown of the cost so the UI can show what's out of zone.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cost {
    pub total: f64,
    pub solvable: bool,
    pub par: f64,
    pub dead_ends: f64,
    pub misdirection: f64,
    pub hazard: f64,
    pub density: f64,
}

impl Cost {
    pub fn in_zone(&self) -> bool {
        self.solvable && self.total < 1e-9
    }
}

pub fn cost(lv: &Level, an: &Analysis, s: &Settings) -> Cost {
    let n = lv.len() as f64;
    let dens = |t: Tile| lv.count(t) as f64 / n;
    let density_zone = |target: f64| Zone::new(target, (target * 0.35).max(0.01));
    let density = 0.5
        * (density_zone(s.rock_density).err(dens(Tile::Rock), 0.05)
            + density_zone(s.floor_density).err(dens(Tile::Floor), 0.05)
            + density_zone(s.hole_density).err(dens(Tile::Hole), 0.05));

    if !an.solvable {
        // Guide towards solvability: how close does the reachable set get to
        // the goal, and is the reachable set big enough to be interesting?
        let gap = an.gap as f64 / (lv.w + lv.h) as f64;
        let co = 1.0 - an.coreach as f64 / n;
        let reach = 1.0 - an.reachable as f64 / n;
        let total = 6.0 + 4.0 * gap + co + reach + density;
        return Cost {
            total,
            solvable: false,
            density,
            ..Default::default()
        };
    }

    let par_scale = (s.par.tol).max(2.0);
    let par = 2.0 * s.par.err(an.par as f64, par_scale);
    let dead_ends = s.dead_ends.err(an.dead_end_ratio(), 0.08);
    let misdirection = s.misdirection.err(an.misdirection_ratio(), 0.1);
    let hazard = if s.holes_enabled() {
        s.hazard.err(an.hazard_ratio(), 0.06)
    } else {
        0.0
    };
    Cost {
        total: par + dead_ends + misdirection + hazard + density,
        solvable: true,
        par,
        dead_ends,
        misdirection,
        hazard,
        density,
    }
}

fn random_cell_except(rng: &mut Rng, lv: &Level) -> usize {
    loop {
        let c = rng.below(lv.len());
        if c != lv.start && c != lv.goal {
            return c;
        }
    }
}

fn random_cell_of(rng: &mut Rng, lv: &Level, t: Tile) -> Option<usize> {
    // Rejection sampling first (fast), fall back to a scan.
    for _ in 0..16 {
        let c = rng.below(lv.len());
        if lv.tiles[c] == t && c != lv.start && c != lv.goal {
            return Some(c);
        }
    }
    let cands: Vec<usize> = (0..lv.len())
        .filter(|&c| lv.tiles[c] == t && c != lv.start && c != lv.goal)
        .collect();
    if cands.is_empty() {
        None
    } else {
        Some(cands[rng.below(cands.len())])
    }
}

/// Build a random starting map.
pub fn random_level(rng: &mut Rng, s: &Settings) -> Level {
    let mut lv = Level::new(s.w, s.h, s.rule);
    for t in lv.tiles.iter_mut() {
        let r = rng.f64();
        *t = if r < s.rock_density {
            Tile::Rock
        } else if r < s.rock_density + s.floor_density {
            Tile::Floor
        } else if r < s.rock_density + s.floor_density + s.hole_density {
            Tile::Hole
        } else {
            Tile::Ice
        };
    }
    lv.start = rng.below(lv.len());
    // Put the goal reasonably far from the start.
    let min_d = (s.w + s.h) / 3;
    loop {
        let g = rng.below(lv.len());
        let (sx, sy) = lv.xy(lv.start);
        let (gx, gy) = lv.xy(g);
        if g != lv.start && sx.abs_diff(gx) + sy.abs_diff(gy) >= min_d {
            lv.goal = g;
            break;
        }
    }
    lv.tiles[lv.start] = Tile::Ice;
    lv.tiles[lv.goal] = Tile::Ice;
    lv
}

/// Apply one random mutation in place.
pub fn mutate(rng: &mut Rng, lv: &mut Level, s: &Settings) {
    let r = rng.f64();
    if r < 0.35 {
        // Toggle rock <-> ice.
        let c = random_cell_except(rng, lv);
        lv.tiles[c] = if lv.tiles[c] == Tile::Rock {
            Tile::Ice
        } else {
            Tile::Rock
        };
    } else if r < 0.62 {
        // Nudge an existing rock to a neighbouring cell (keeps density).
        if let Some(c) = random_cell_of(rng, lv, Tile::Rock) {
            let d = crate::level::Dir::ALL[rng.below(4)];
            if let Some(n) = lv.step(c, d)
                && n != lv.start && n != lv.goal {
                    lv.tiles.swap(c, n);
                }
        }
    } else if r < 0.72 {
        // Teleport a rock anywhere.
        if let Some(c) = random_cell_of(rng, lv, Tile::Rock) {
            let n = random_cell_except(rng, lv);
            lv.tiles.swap(c, n);
        }
    } else if r < 0.80 {
        if s.floors_enabled() {
            let c = random_cell_except(rng, lv);
            lv.tiles[c] = if lv.tiles[c] == Tile::Floor {
                Tile::Ice
            } else {
                Tile::Floor
            };
        } else {
            mutate(rng, lv, s);
        }
    } else if r < 0.88 {
        if s.holes_enabled() {
            let c = random_cell_except(rng, lv);
            lv.tiles[c] = if lv.tiles[c] == Tile::Hole {
                Tile::Ice
            } else {
                Tile::Hole
            };
        } else {
            mutate(rng, lv, s);
        }
    } else {
        // Move the start or the goal (to an ice cell).
        if let Some(c) = random_cell_of(rng, lv, Tile::Ice) {
            if rng.chance(0.5) {
                lv.start = c;
            } else {
                lv.goal = c;
            }
        }
    }
    // Clean up tiles the current mode doesn't allow.
    if !s.holes_enabled() || !s.floors_enabled() {
        for t in lv.tiles.iter_mut() {
            if (*t == Tile::Hole && !s.holes_enabled()) || (*t == Tile::Floor && !s.floors_enabled()) {
                *t = Tile::Ice;
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Running,
    /// Finished; `true` if the best map is inside every zone.
    Done(bool),
}

/// Incremental annealer: call [`Annealer::step`] repeatedly (e.g. a few
/// hundred iterations per frame) so the UI can animate the search.
pub struct Annealer {
    pub settings: Settings,
    pub seed: u64,
    rng: Rng,
    pub current: Level,
    pub current_cost: Cost,
    pub best: Level,
    pub best_cost: Cost,
    pub best_analysis: Analysis,
    pub iter: u32,
    pub accepted: u32,
    pub restarts: u32,
    /// Sampled (iteration, current cost, best cost) for plotting.
    pub history: Vec<(u32, f64, f64)>,
    since_improve: u32,
    pub status: Status,
    t0: f64,
    t1: f64,
}

impl Annealer {
    pub fn new(settings: Settings, seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let lv = random_level(&mut rng, &settings);
        let an = analyze(&lv);
        let c = cost(&lv, &an, &settings);
        Annealer {
            settings,
            seed,
            rng,
            best: lv.clone(),
            best_cost: c,
            best_analysis: an,
            current: lv,
            current_cost: c,
            iter: 0,
            accepted: 0,
            restarts: 0,
            history: vec![(0, c.total, c.total)],
            since_improve: 0,
            status: if c.in_zone() {
                Status::Done(true)
            } else {
                Status::Running
            },
            t0: 2.0,
            t1: 0.005,
        }
    }

    pub fn temperature(&self) -> f64 {
        // Geometric cooling over a "cycle"; restarts re-heat.
        let cycle = (self.settings.max_iters / 4).max(1);
        let f = (self.iter % cycle) as f64 / cycle as f64;
        self.t0 * (self.t1 / self.t0).powf(f)
    }

    pub fn progress(&self) -> f64 {
        self.iter as f64 / self.settings.max_iters.max(1) as f64
    }

    pub fn step(&mut self, n: u32) -> Status {
        if self.status != Status::Running {
            return self.status;
        }
        for _ in 0..n {
            self.iter += 1;
            let t = self.temperature();
            let mut cand = self.current.clone();
            // Hotter -> bigger jumps.
            let k = 1 + (t * 1.5) as usize + usize::from(self.rng.chance(0.2));
            for _ in 0..k {
                mutate(&mut self.rng, &mut cand, &self.settings);
            }
            let an = analyze(&cand);
            let c = cost(&cand, &an, &self.settings);
            let delta = c.total - self.current_cost.total;
            if delta <= 0.0 || self.rng.f64() < (-delta / t).exp() {
                self.current = cand;
                self.current_cost = c;
                self.accepted += 1;
                if c.total < self.best_cost.total {
                    self.best = self.current.clone();
                    self.best_cost = c;
                    self.best_analysis = an;
                    self.since_improve = 0;
                }
            }
            self.since_improve += 1;

            if self.iter.is_multiple_of(50) {
                self.history
                    .push((self.iter, self.current_cost.total, self.best_cost.total));
            }
            if self.best_cost.in_zone() {
                self.status = Status::Done(true);
                break;
            }
            // Stuck for a long time? Jump back to the best map and re-heat.
            if self.since_improve > self.settings.max_iters / 8 {
                if self.best_cost.solvable {
                    self.current = self.best.clone();
                    self.current_cost = self.best_cost;
                } else {
                    // Never found anything solvable: start over from scratch.
                    self.current = random_level(&mut self.rng, &self.settings);
                    let an = analyze(&self.current);
                    self.current_cost = cost(&self.current, &an, &self.settings);
                }
                self.since_improve = 0;
                self.restarts += 1;
            }
            if self.iter >= self.settings.max_iters {
                self.status = Status::Done(self.best_cost.in_zone());
                break;
            }
        }
        self.status
    }

    /// Run to completion.
    pub fn run(&mut self) -> Status {
        loop {
            if let Status::Done(z) = self.step(1000) {
                return Status::Done(z);
            }
        }
    }
}

/// Convenience: generate a level synchronously.
pub fn generate(settings: &Settings, seed: u64) -> (Level, Analysis, Cost, bool) {
    let mut a = Annealer::new(settings.clone(), seed);
    let st = a.run();
    (
        a.best.clone(),
        a.best_analysis.clone(),
        a.best_cost,
        st == Status::Done(true),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zone_error() {
        let z = Zone::new(10.0, 2.0);
        assert_eq!(z.err(9.0, 1.0), 0.0);
        assert_eq!(z.err(12.0, 1.0), 0.0);
        assert!(z.err(13.0, 1.0) > 0.0);
        assert!(z.err(14.0, 1.0) > z.err(13.0, 1.0));
        assert!(z.err(5.0, 1.0) > 0.0);
    }

    #[test]
    fn mutations_keep_start_and_goal_on_ice() {
        let s = Settings::from_difficulty(8.0);
        let mut rng = Rng::new(1);
        let mut lv = random_level(&mut rng, &s);
        for _ in 0..5000 {
            mutate(&mut rng, &mut lv, &s);
            assert_eq!(lv.tiles[lv.start], Tile::Ice);
            assert_eq!(lv.tiles[lv.goal], Tile::Ice);
            assert_ne!(lv.start, lv.goal);
        }
    }

    #[test]
    fn generation_is_deterministic() {
        let mut s = Settings::from_difficulty(4.0);
        s.max_iters = 3000;
        let a = generate(&s, 99);
        let b = generate(&s, 99);
        assert_eq!(a.0, b.0);
    }

    #[test]
    fn hits_the_zone_across_difficulties() {
        for d in [1.0, 3.0, 5.0, 7.0, 10.0] {
            let s = Settings::from_difficulty(d);
            let mut hits = 0;
            for seed in 0..4 {
                let (lv, an, c, ok) = generate(&s, seed);
                assert!(an.solvable, "d={d} seed={seed}\n{lv}");
                if ok {
                    hits += 1;
                    assert!(s.par.contains(an.par as f64), "{c:?}");
                }
            }
            assert!(hits >= 3, "difficulty {d}: only {hits}/4 seeds reached the zone");
        }
    }

    #[test]
    fn harder_settings_give_harder_maps() {
        let easy = generate(&Settings::from_difficulty(2.0), 5).1;
        let hard = generate(&Settings::from_difficulty(9.0), 5).1;
        assert!(hard.par > easy.par);
        assert!(hard.difficulty() > easy.difficulty());
    }
}
