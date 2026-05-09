// Simulate one year of hourly solar PV generation:
//   - clear-sky model from solar declination + elevation
//   - seasonal mean clearness (summer clearer than winter)
//   - fractal (multi-octave value noise) weather perturbation so cloud
//     conditions roll over multi-day fronts instead of jittering hourly
// Output: CSV with 8760 rows (8784 in a leap year): timestamp, ghi, power.
//
// Usage: simulate_solar [lat] [year] [peak_kw] [out.csv]
//        defaults: 40.4168, 2020, 5.0, solar_simulated.csv

use std::env;
use std::f64::consts::PI;
use std::fs::File;
use std::io::{BufWriter, Write};

const DAYS_PER_MONTH: [u32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
const SYSTEM_DERATE: f64 = 0.86;
const CLEAR_SKY_PEAK_W_M2: f64 = 1000.0;

// Year-round mean clearness index (1.0 = clear sky, 0.0 = totally overcast).
const MEAN_CLEARNESS: f64 = 0.70;
// Half-amplitude of the seasonal swing around MEAN_CLEARNESS.
const SEASONAL_AMPLITUDE: f64 = 0.20;
// Half-amplitude of fractal weather perturbation on top of the seasonal mean.
const WEATHER_AMPLITUDE: f64 = 0.30;

// Octave periods (hours) for the fBm weather noise. Largest = synoptic
// systems (~10 days); smallest = within-day cloud passage.
const OCTAVE_PERIODS_H: [f64; 5] = [240.0, 96.0, 36.0, 12.0, 4.0];

fn is_leap(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

// Cooper's equation: solar declination angle (radians) for day-of-year.
fn solar_declination(doy: u32) -> f64 {
    23.45_f64.to_radians() * (2.0 * PI * (284.0 + doy as f64) / 365.0).sin()
}

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

// 1-D value noise: random "knots" placed every `period` hours, smoothly
// interpolated. Returns values in roughly [-1, 1].
struct ValueNoise1D {
    knots: Vec<f64>,
    period: f64,
}

impl ValueNoise1D {
    fn new(span_hours: usize, period: f64, state: &mut u64) -> Self {
        let n = (span_hours as f64 / period).ceil() as usize + 2;
        let knots = (0..n).map(|_| rand_unit(state) * 2.0 - 1.0).collect();
        Self { knots, period }
    }

    fn sample(&self, t: f64) -> f64 {
        let x = t / self.period;
        let i = x.floor() as usize;
        let f = x - i as f64;
        let s = f * f * (3.0 - 2.0 * f); // smoothstep
        self.knots[i] * (1.0 - s) + self.knots[i + 1] * s
    }
}

// Fractional Brownian motion: sum octaves with halving amplitude (pink-ish).
fn fbm(t: f64, octaves: &[ValueNoise1D]) -> f64 {
    let mut sum = 0.0;
    let mut amp = 1.0;
    let mut norm = 0.0;
    for o in octaves {
        sum += amp * o.sample(t);
        norm += amp;
        amp *= 0.5;
    }
    sum / norm
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

    let total_hours = if is_leap(year) { 366 * 24 } else { 365 * 24 };
    let octaves: Vec<ValueNoise1D> = OCTAVE_PERIODS_H
        .iter()
        .map(|&p| ValueNoise1D::new(total_hours, p, &mut rng))
        .collect();

    // Northern-hemisphere summer peaks at doy 172; flip for the south.
    let peak_clear_doy: f64 = if lat < 0.0 { 355.0 } else { 172.0 };

    let mut doy: u32 = 0;
    let mut hour_index: u64 = 0;
    let mut rows: u64 = 0;
    for (m_idx, &dim) in DAYS_PER_MONTH.iter().enumerate() {
        let month = (m_idx + 1) as u32;
        let days_in_month = if month == 2 && is_leap(year) { 29 } else { dim };
        for day in 1..=days_in_month {
            doy += 1;
            let decl = solar_declination(doy);
            let season = (2.0 * PI * (doy as f64 - peak_clear_doy) / 365.25).cos();
            let mean_clear = MEAN_CLEARNESS + SEASONAL_AMPLITUDE * season;

            for hour in 0..24u32 {
                let ha = ((hour as f64 + 0.5) - 12.0) * 15.0_f64.to_radians();
                let sin_elev =
                    lat_rad.sin() * decl.sin() + lat_rad.cos() * decl.cos() * ha.cos();
                let elev = sin_elev.max(0.0);
                let clearsky = CLEAR_SKY_PEAK_W_M2 * elev;

                let weather = fbm(hour_index as f64, &octaves);
                let clearness =
                    (mean_clear + WEATHER_AMPLITUDE * weather).clamp(0.05, 1.0);
                let ghi = clearsky * clearness;
                let power = peak_kw * 1000.0 * (ghi / CLEAR_SKY_PEAK_W_M2) * SYSTEM_DERATE;

                writeln!(
                    w,
                    "{:04}-{:02}-{:02}T{:02}:00:00,{:.1},{:.1}",
                    year, month, day, hour, ghi, power
                )?;
                rows += 1;
                hour_index += 1;
            }
        }
    }

    println!("wrote {} ({} rows)", out, rows);
    Ok(())
}
