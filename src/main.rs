// Simulate one year of hourly solar PV generation using a simple clear-sky
// model (solar declination + elevation) with multiplicative cloud noise.
// Output: CSV with 8760 rows (8784 in a leap year): timestamp, ghi, power.
//
// Usage: simulate_solar [lat] [year] [peak_kw] [out.csv]
//        defaults: 40.4168, 2020, 5.0, solar_simulated.csv

use std::env;
use std::f64::consts::PI;
use std::fs::File;
use std::io::{BufWriter, Write};

const DAYS_PER_MONTH: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
const SYSTEM_DERATE: f64 = 0.86; // 14% combined inverter/wiring/soiling loss
const CLEAR_SKY_PEAK_W_M2: f64 = 1000.0;

fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

// Cooper's equation: solar declination angle (radians) for day-of-year.
fn solar_declination(doy: u32) -> f64 {
    23.45_f64.to_radians() * (2.0 * PI * (284.0 + doy as f64) / 365.0).sin()
}

// xorshift64 — deterministic pseudo-random for reproducible cloud noise.
fn next_u64(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

fn rand_unit(state: &mut u64) -> f64 {
    (next_u64(state) >> 11) as f64 / (1u64 << 53) as f64
}

fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();
    let lat: f64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(40.4168);
    let year: i32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(2020);
    let peak_kw: f64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(5.0);
    let out: String = args
        .get(4)
        .cloned()
        .unwrap_or_else(|| "solar_simulated.csv".into());

    let lat_rad = lat.to_radians();
    let mut w = BufWriter::new(File::create(&out)?);
    writeln!(w, "timestamp,ghi_w_m2,power_w")?;

    let mut rng: u64 = 0xDEAD_BEEF_CAFE_BABE_u64
        .wrapping_add(year as u64)
        .wrapping_add((lat.abs() * 1000.0) as u64);
    if rng == 0 {
        rng = 1;
    }

    let mut doy: u32 = 0;
    let mut rows: u64 = 0;
    for (m_idx, &dim) in DAYS_PER_MONTH.iter().enumerate() {
        let month = (m_idx + 1) as u32;
        let days_in_month = if month == 2 && is_leap(year) { 29 } else { dim };
        for day in 1..=days_in_month {
            doy += 1;
            let decl = solar_declination(doy);
            for hour in 0..24u32 {
                // Centre on the half-hour so we aren't sampling sun-up moments at the edges.
                let ha = ((hour as f64 + 0.5) - 12.0) * 15.0_f64.to_radians();
                let sin_elev =
                    lat_rad.sin() * decl.sin() + lat_rad.cos() * decl.cos() * ha.cos();
                let elev = sin_elev.max(0.0);
                let clearsky = CLEAR_SKY_PEAK_W_M2 * elev;
                let cloud = 0.5 + 0.5 * rand_unit(&mut rng);
                let ghi = clearsky * cloud;
                let power = peak_kw * 1000.0 * (ghi / CLEAR_SKY_PEAK_W_M2) * SYSTEM_DERATE;
                writeln!(
                    w,
                    "{:04}-{:02}-{:02}T{:02}:00:00,{:.1},{:.1}",
                    year, month, day, hour, ghi, power
                )?;
                rows += 1;
            }
        }
    }

    println!("wrote {} ({} rows)", out, rows);
    Ok(())
}
