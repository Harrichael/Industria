//! Headless generator: prints maps and their metrics.
//!
//! ```text
//! cargo run --release --bin icegen -- [difficulty=5] [count=1] [seed=1]
//! ```

use ice_slide::generator::{Annealer, Settings, Status};
use ice_slide::level::GoalRule;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let d: f64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(5.0);
    let count: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
    let seed0: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);
    let s = Settings::from_difficulty(d);
    println!(
        "difficulty {d}: {}x{} {:?} par {:.0}..{:.0} dead {:.2}..{:.2} misdir {:.2}..{:.2} hazard {:.2}..{:.2}",
        s.w, s.h, s.rule, s.par.lo(), s.par.hi(), s.dead_ends.lo(), s.dead_ends.hi(),
        s.misdirection.lo(), s.misdirection.hi(), s.hazard.lo(), s.hazard.hi()
    );
    for seed in seed0..seed0 + count {
        let t = std::time::Instant::now();
        let mut a = Annealer::new(s.clone(), seed);
        let st = a.run();
        let an = &a.best_analysis;
        if count == 1 {
            print!("{}", a.best);
            let sol: String = an.solution.iter().map(|d| d.arrow()).collect();
            println!("solution: {sol}");
        }
        println!(
            "seed {seed:>3} {} iters {:>6} ({:>4} ms) par {:>2} dead {:.2} misdir {:.2} hazard {:.2} decisions {:>2} paths {:>3} reach {:>3} -> score {:.1}{}  cost {:?}",
            if st == Status::Done(true) { "IN ZONE " } else { "missed  " },
            a.iter, t.elapsed().as_millis(), an.par, an.dead_end_ratio(), an.misdirection_ratio(),
            an.hazard_ratio(), an.decisions, an.optimal_paths, an.reachable, an.difficulty(),
            if a.best.rule == GoalRule::MustStop { "" } else { " (pass-through)" },
            (a.best_cost.par, a.best_cost.dead_ends, a.best_cost.misdirection, a.best_cost.hazard, a.best_cost.density)
        );
    }
}
