// Sweep the (generation_multiple, battery_days) plane against an hourly
// solar generation CSV and emit the Pareto frontier — the minimum
// generation multiplier that meets a constant base load all year, for
// each candidate battery size. A "battery day" stores enough energy to
// run the entire base load for 24 hours.
//
// After the sweep, run a golden-section search over battery_days to find
// the precise cost-minimum configuration (the inner gen_mult is found by
// the same binary search the sweep uses). Reported on stderr.
//
// Usage:
//   analyze_solar --base-load <W> --cost <file> --solar <csv> \
//                 [--output pareto.csv] [--max-gen 10] \
//                 [--max-battery-days 14] [--supply-percent 100]

use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::process::ExitCode;

struct Args {
    base_load_w: f64,
    cost_path: String,
    solar_path: String,
    out_path: String,
    max_gen: f64,
    max_battery_days: f64,
    supply_percent: f64,
}

fn parse_args() -> Result<Args, String> {
    let mut base_load_w: Option<f64> = None;
    let mut cost_path: Option<String> = None;
    let mut solar_path: Option<String> = None;
    let mut out_path: String = "pareto.csv".into();
    let mut max_gen: f64 = 10.0;
    let mut max_battery_days: f64 = 14.0;
    let mut supply_percent: f64 = 100.0;

    let mut it = env::args().skip(1);
    while let Some(arg) = it.next() {
        let take = |opt: Option<String>| opt.ok_or_else(|| format!("missing value for {arg}"));
        match arg.as_str() {
            "--base-load" => base_load_w = Some(take(it.next())?.parse().map_err(|e| format!("--base-load: {e}"))?),
            "--cost" => cost_path = Some(take(it.next())?),
            "--solar" => solar_path = Some(take(it.next())?),
            "--output" | "-o" => out_path = take(it.next())?,
            "--max-gen" => max_gen = take(it.next())?.parse().map_err(|e| format!("--max-gen: {e}"))?,
            "--max-battery-days" => max_battery_days = take(it.next())?.parse().map_err(|e| format!("--max-battery-days: {e}"))?,
            "--supply-percent" => supply_percent = take(it.next())?.parse().map_err(|e| format!("--supply-percent: {e}"))?,
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            other => return Err(format!("unknown arg: {other}")),
        }
    }

    if !(0.0 < supply_percent && supply_percent <= 100.0) {
        return Err(format!(
            "--supply-percent must be in (0, 100], got {supply_percent}"
        ));
    }

    Ok(Args {
        base_load_w: base_load_w.ok_or("--base-load is required")?,
        cost_path: cost_path.ok_or("--cost is required")?,
        solar_path: solar_path.ok_or("--solar is required")?,
        out_path,
        max_gen,
        max_battery_days,
        supply_percent,
    })
}

fn print_usage() {
    eprintln!(
        "Usage: analyze_solar --base-load <W> --cost <file> --solar <csv>\n\
         \x20             [--output pareto.csv] [--max-gen 10]\n\
         \x20             [--max-battery-days 14] [--supply-percent 100]\n\
         \n\
         --supply-percent: percentage of grid demand the solar+battery system\n\
         must cover (the remainder is assumed to come from another source)."
    );
}

// Read the third (power_w) column of the solar CSV, returning hourly W.
fn read_solar(path: &str) -> Result<Vec<f64>, String> {
    let file = File::open(path).map_err(|e| format!("open {path}: {e}"))?;
    let mut out = Vec::new();
    for (i, line) in BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|e| format!("read {path}:{}: {e}", i + 1))?;
        if i == 0 || line.trim().is_empty() {
            continue;
        }
        let last = line
            .rsplit(',')
            .next()
            .ok_or_else(|| format!("{path}:{}: empty row", i + 1))?;
        let v: f64 = last
            .trim()
            .parse()
            .map_err(|e| format!("{path}:{}: parse power: {e}", i + 1))?;
        out.push(v);
    }
    if out.is_empty() {
        return Err(format!("{path}: no data rows"));
    }
    Ok(out)
}

fn read_cost(path: &str) -> Result<HashMap<String, f64>, String> {
    let file = File::open(path).map_err(|e| format!("open {path}: {e}"))?;
    let mut map = HashMap::new();
    for (i, line) in BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|e| format!("read {path}:{}: {e}", i + 1))?;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let (k, v) = trimmed
            .split_once('=')
            .ok_or_else(|| format!("{path}:{}: expected key = value", i + 1))?;
        let value: f64 = v
            .trim()
            .parse()
            .map_err(|e| format!("{path}:{}: parse value: {e}", i + 1))?;
        map.insert(k.trim().to_string(), value);
    }
    for required in ["generation_per_multiple", "battery_per_day"] {
        if !map.contains_key(required) {
            return Err(format!("{path}: missing required key `{required}`"));
        }
    }
    Ok(map)
}

// Simulate one year. Returns the unmet-energy fraction of total demand.
// Battery starts full and is hard-clamped to [0, capacity] each hour.
fn simulate(solar_w: &[f64], base_load_w: f64, gen_mult: f64, battery_days: f64) -> f64 {
    let capacity_wh = base_load_w * 24.0 * battery_days;
    let mut soc = capacity_wh;
    let mut unmet_wh = 0.0;
    for &p in solar_w {
        let gen_wh = p * gen_mult;
        let net = gen_wh - base_load_w;
        if net >= 0.0 {
            soc = (soc + net).min(capacity_wh);
        } else {
            let need = -net;
            if soc >= need {
                soc -= need;
            } else {
                unmet_wh += need - soc;
                soc = 0.0;
            }
        }
    }
    let demand_wh = base_load_w * solar_w.len() as f64;
    unmet_wh / demand_wh
}

// Binary search for the smallest gen_mult in [0, max_gen] whose deficit
// is within tolerance. Returns None if even max_gen is infeasible.
fn min_gen_for(
    solar_w: &[f64],
    base_load_w: f64,
    battery_days: f64,
    max_gen: f64,
    tol: f64,
) -> Option<f64> {
    if simulate(solar_w, base_load_w, max_gen, battery_days) > tol {
        return None;
    }
    let mut lo = 0.0_f64;
    let mut hi = max_gen;
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        if simulate(solar_w, base_load_w, mid, battery_days) <= tol {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    Some(hi)
}

// Smallest battery_days in [0, max_days] that admits a feasible gen_mult,
// found by bisection. Returns None if even max_days is infeasible.
fn min_feasible_battery(
    solar_w: &[f64],
    base_load_w: f64,
    max_gen: f64,
    max_days: f64,
    tol: f64,
) -> Option<f64> {
    if min_gen_for(solar_w, base_load_w, max_days, max_gen, tol).is_none() {
        return None;
    }
    if min_gen_for(solar_w, base_load_w, 0.0, max_gen, tol).is_some() {
        return Some(0.0);
    }
    let mut lo = 0.0_f64;
    let mut hi = max_days;
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        if min_gen_for(solar_w, base_load_w, mid, max_gen, tol).is_some() {
            hi = mid;
        } else {
            lo = mid;
        }
    }
    Some(hi)
}

// Golden-section search for the battery_days that minimises total cost,
// on the assumption that cost(battery_days) is unimodal across the
// feasible interval (matches the U-shape we observe in practice).
fn find_cost_optimum(
    solar_w: &[f64],
    base_load_w: f64,
    max_gen: f64,
    max_days: f64,
    tol: f64,
    gen_cost: f64,
    bat_cost: f64,
) -> Option<(f64, f64, f64)> {
    let bd_lo = min_feasible_battery(solar_w, base_load_w, max_gen, max_days, tol)?;
    let cost_at = |bd: f64| -> Option<(f64, f64)> {
        min_gen_for(solar_w, base_load_w, bd, max_gen, tol)
            .map(|g| (g, gen_cost * g + bat_cost * bd))
    };

    let phi: f64 = (1.0 + 5.0_f64.sqrt()) / 2.0;
    let resphi: f64 = 2.0 - phi; // 1 - 1/phi ≈ 0.382
    let mut a = bd_lo;
    let mut b = max_days;
    if (b - a) < 1e-9 {
        let (g, cost) = cost_at(a)?;
        return Some((a, g, cost));
    }
    let mut c = a + resphi * (b - a);
    let mut d = b - resphi * (b - a);
    let mut fc = cost_at(c)?.1;
    let mut fd = cost_at(d)?.1;

    for _ in 0..60 {
        if (b - a) < 1e-4 {
            break;
        }
        if fc < fd {
            b = d;
            d = c;
            fd = fc;
            c = a + resphi * (b - a);
            fc = cost_at(c)?.1;
        } else {
            a = c;
            c = d;
            fc = fd;
            d = b - resphi * (b - a);
            fd = cost_at(d)?.1;
        }
    }

    let bd_star = 0.5 * (a + b);
    let (g_star, cost_star) = cost_at(bd_star)?;
    Some((bd_star, g_star, cost_star))
}

fn battery_grid(max_days: f64) -> Vec<f64> {
    // Geometric sweep from a quarter-day up to max, plus 0 for completeness.
    let mut grid = vec![0.0_f64];
    let mut x = 0.25_f64;
    let step = 2.0_f64.sqrt();
    while x <= max_days + 1e-9 {
        grid.push(x);
        x *= step;
    }
    grid
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let cost = read_cost(&args.cost_path)?;
    let solar = read_solar(&args.solar_path)?;
    let deficit_tolerance = 1.0 - args.supply_percent / 100.0;
    eprintln!(
        "loaded {} hourly samples; base_load = {} W; supply target = {}%",
        solar.len(),
        args.base_load_w,
        args.supply_percent
    );

    let gen_cost = cost["generation_per_multiple"];
    let bat_cost = cost["battery_per_day"];

    let mut writer = BufWriter::new(
        File::create(&args.out_path).map_err(|e| format!("create {}: {e}", args.out_path))?,
    );
    writeln!(
        writer,
        "battery_days,generation_multiple,total_cost,unmet_fraction"
    )
    .map_err(|e| e.to_string())?;

    let mut wrote = 0usize;
    for &bd in &battery_grid(args.max_battery_days) {
        match min_gen_for(
            &solar,
            args.base_load_w,
            bd,
            args.max_gen,
            deficit_tolerance,
        ) {
            Some(g) => {
                let total_cost = gen_cost * g + bat_cost * bd;
                let unmet = simulate(&solar, args.base_load_w, g, bd);
                writeln!(
                    writer,
                    "{:.4},{:.4},{:.2},{:.6}",
                    bd, g, total_cost, unmet
                )
                .map_err(|e| e.to_string())?;
                wrote += 1;
            }
            None => {
                eprintln!(
                    "battery_days = {:.4}: infeasible at max_gen = {}",
                    bd, args.max_gen
                );
            }
        }
    }

    eprintln!("wrote {} pareto points to {}", wrote, args.out_path);

    match find_cost_optimum(
        &solar,
        args.base_load_w,
        args.max_gen,
        args.max_battery_days,
        deficit_tolerance,
        gen_cost,
        bat_cost,
    ) {
        Some((bd, g, cost)) => {
            let unmet = simulate(&solar, args.base_load_w, g, bd);
            eprintln!(
                "cost-minimum: battery_days={:.4}, generation_multiple={:.4}, total_cost={:.2}, unmet_fraction={:.6}",
                bd, g, cost, unmet
            );
        }
        None => {
            eprintln!(
                "cost-minimum: no feasible configuration in [0, {}] battery_days at max_gen={}",
                args.max_battery_days, args.max_gen
            );
        }
    }

    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
