//! Ice Slide — the playable game.

mod ui;

use ice_slide::generator::{Annealer, Settings, Status, Zone};
use ice_slide::level::{Dir, GoalRule, Level, Outcome, Tile};
use ice_slide::solver::{analyze, hint, Analysis, UNREACHABLE};
use macroquad::prelude::*;
use ui::{round_rect, text, text_width, theme, Ui};

const PANEL_W: f32 = 340.0;

fn window_conf() -> Conf {
    Conf {
        window_title: "Ice Slide".to_owned(),
        window_width: 1280,
        window_height: 860,
        window_resizable: true,
        high_dpi: true,
        sample_count: 4,
        ..Default::default()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PlayState {
    Playing,
    Won,
    Sank,
}

struct Slide {
    path: Vec<usize>,
    outcome: Outcome,
    t: f32,
}

struct Game {
    settings: Settings,
    difficulty: f64,
    seed: u64,
    search: Option<Annealer>,
    gen_started: f64,
    gen_time: f64,
    level: Level,
    an: Analysis,
    in_zone: bool,
    history: Vec<(u32, f64, f64)>,
    gen_iters: u32,

    pos: usize,
    undo: Vec<usize>,
    state: PlayState,
    slide: Option<Slide>,
    queued: Option<Dir>,
    facing: Dir,
    trail: Vec<(Vec2, f32)>,
    hints_used: u32,
    show_hint: bool,
    show_solution: bool,
    warn_dead_ends: bool,
    show_panel: bool,
    advanced: bool,
    best_moves: Option<usize>,

    swipe_from: Option<Vec2>,
    ui: Ui,
    flash: f32,
}

impl Game {
    fn new() -> Self {
        let difficulty = 4.0;
        let settings = Settings::from_difficulty(difficulty);
        let seed = (miniquad::date::now() * 1000.0) as u64 % 1_000_000;
        let level = Level::new(settings.w, settings.h, settings.rule);
        let an = analyze(&level);
        let mut g = Game {
            settings,
            difficulty,
            seed,
            search: None,
            gen_started: 0.0,
            gen_time: 0.0,
            pos: level.start,
            level,
            an,
            in_zone: false,
            history: Vec::new(),
            gen_iters: 0,
            undo: Vec::new(),
            state: PlayState::Playing,
            slide: None,
            queued: None,
            facing: Dir::Down,
            trail: Vec::new(),
            hints_used: 0,
            show_hint: false,
            show_solution: false,
            warn_dead_ends: true,
            show_panel: screen_width() > 900.0,
            advanced: false,
            best_moves: None,
            swipe_from: None,
            ui: Ui::default(),
            flash: 0.0,
        };
        g.start_generation();
        g
    }

    fn start_generation(&mut self) {
        self.search = Some(Annealer::new(self.settings.clone(), self.seed));
        self.gen_started = get_time();
        self.slide = None;
        self.queued = None;
        self.show_solution = false;
        self.show_hint = false;
    }

    fn new_level(&mut self) {
        self.seed = (self.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) >> 33) % 1_000_000;
        self.start_generation();
    }

    fn reset_play(&mut self) {
        self.pos = self.level.start;
        self.undo.clear();
        self.state = PlayState::Playing;
        self.slide = None;
        self.queued = None;
        self.trail.clear();
        self.facing = Dir::Down;
    }

    fn update_generation(&mut self) {
        let Some(search) = self.search.as_mut() else { return };
        // Spend a slice of each frame annealing so the search is visible but
        // the UI stays responsive.
        let budget = 0.012;
        let t0 = get_time();
        let mut st = Status::Running;
        while get_time() - t0 < budget {
            st = search.step(60);
            if st != Status::Running {
                break;
            }
        }
        if let Status::Done(ok) = st {
            self.level = search.best.clone();
            self.an = search.best_analysis.clone();
            self.in_zone = ok;
            self.history = search.history.clone();
            self.gen_iters = search.iter;
            self.gen_time = get_time() - self.gen_started;
            self.search = None;
            self.hints_used = 0;
            self.best_moves = None;
            self.reset_play();
        }
    }

    fn try_move(&mut self, d: Dir) {
        if self.search.is_some() || self.state != PlayState::Playing {
            return;
        }
        if self.slide.is_some() {
            self.queued = Some(d);
            return;
        }
        let (out, path) = self.level.slide_path(self.pos, d);
        self.facing = d;
        if path.len() <= 1 {
            self.flash = 0.25; // bump
            return;
        }
        self.undo.push(self.pos);
        self.show_hint = false;
        self.slide = Some(Slide { path, outcome: out, t: 0.0 });
    }

    fn do_undo(&mut self) {
        if self.search.is_some() {
            return;
        }
        if let Some(p) = self.undo.pop() {
            self.pos = p;
            self.slide = None;
            self.queued = None;
            self.state = PlayState::Playing;
            self.trail.clear();
        }
    }

    fn update_slide(&mut self, dt: f32) {
        let Some(sl) = self.slide.as_mut() else { return };
        let cells = (sl.path.len() - 1) as f32;
        // Accelerate a little on long slides.
        let speed = 14.0 + cells * 1.2;
        sl.t += dt * speed;
        if sl.t >= cells {
            let out = sl.outcome;
            self.pos = out.cell();
            self.slide = None;
            match out {
                Outcome::Won(_) => {
                    self.state = PlayState::Won;
                    let m = self.undo.len();
                    self.best_moves = Some(self.best_moves.map_or(m, |b| b.min(m)));
                }
                Outcome::Sank(_) => self.state = PlayState::Sank,
                Outcome::Stopped(_) => {
                    if let Some(q) = self.queued.take() {
                        self.try_move(q);
                    }
                }
            }
        }
    }

    fn player_cell_f(&self) -> (f32, f32) {
        if let Some(sl) = &self.slide {
            let i = (sl.t.floor() as usize).min(sl.path.len() - 1);
            let j = (i + 1).min(sl.path.len() - 1);
            let f = sl.t - i as f32;
            let (ax, ay) = self.level.xy(sl.path[i]);
            let (bx, by) = self.level.xy(sl.path[j]);
            (
                ax as f32 + (bx as f32 - ax as f32) * f,
                ay as f32 + (by as f32 - ay as f32) * f,
            )
        } else {
            let (x, y) = self.level.xy(self.pos);
            (x as f32, y as f32)
        }
    }

    fn handle_keys(&mut self) {
        let dirs = [
            (KeyCode::Up, Dir::Up),
            (KeyCode::W, Dir::Up),
            (KeyCode::Down, Dir::Down),
            (KeyCode::S, Dir::Down),
            (KeyCode::Left, Dir::Left),
            (KeyCode::A, Dir::Left),
            (KeyCode::Right, Dir::Right),
            (KeyCode::D, Dir::Right),
        ];
        for (k, d) in dirs {
            if is_key_pressed(k) {
                self.try_move(d);
            }
        }
        if is_key_pressed(KeyCode::U) || is_key_pressed(KeyCode::Z) || is_key_pressed(KeyCode::Backspace) {
            self.do_undo();
        }
        if is_key_pressed(KeyCode::R) && self.search.is_none() {
            self.reset_play();
        }
        if is_key_pressed(KeyCode::N) || (is_key_pressed(KeyCode::Enter) && self.state == PlayState::Won) {
            self.new_level();
        }
        if is_key_pressed(KeyCode::H) {
            self.toggle_hint();
        }
        if is_key_pressed(KeyCode::J) {
            self.auto_step();
        }
        if is_key_pressed(KeyCode::P) {
            self.show_solution = !self.show_solution;
        }
        if is_key_pressed(KeyCode::Tab) {
            self.show_panel = !self.show_panel;
        }
        if is_key_pressed(KeyCode::Space) {
            // Finish the search instantly.
            if let Some(g) = self.search.as_mut() {
                g.run();
            }
        }
        let num = [
            KeyCode::Key1, KeyCode::Key2, KeyCode::Key3, KeyCode::Key4, KeyCode::Key5,
            KeyCode::Key6, KeyCode::Key7, KeyCode::Key8, KeyCode::Key9, KeyCode::Key0,
        ];
        for (i, k) in num.iter().enumerate() {
            if is_key_pressed(*k) {
                self.set_difficulty((i + 1) as f64);
                self.new_level();
            }
        }
    }

    fn toggle_hint(&mut self) {
        if self.search.is_none() && self.state == PlayState::Playing {
            if !self.show_hint {
                self.hints_used += 1;
            }
            self.show_hint = !self.show_hint;
        }
    }

    /// Make the next optimal move for the player (counts as a hint).
    fn auto_step(&mut self) {
        if self.search.is_some() || self.state != PlayState::Playing || self.slide.is_some() {
            return;
        }
        if let Some(d) = hint(&self.level, &self.an, self.pos) {
            self.hints_used += 1;
            self.try_move(d);
        }
    }

    fn set_difficulty(&mut self, d: f64) {
        self.difficulty = d;
        self.settings = Settings::from_difficulty(d);
    }

    // ------------------------------------------------------------------
    // Drawing
    // ------------------------------------------------------------------

    fn board_rect(&self) -> Rect {
        let left = if self.show_panel { PANEL_W } else { 0.0 };
        let top = 64.0;
        let bottom = 44.0;
        let area = Rect::new(left + 24.0, top, screen_width() - left - 48.0, screen_height() - top - bottom);
        let (w, h) = (self.shown_level().w as f32, self.shown_level().h as f32);
        let cell = (area.w / w).min(area.h / h).floor().max(6.0);
        let bw = cell * w;
        let bh = cell * h;
        Rect::new(
            (area.x + (area.w - bw) / 2.0).round(),
            (area.y + (area.h - bh) / 2.0).round(),
            bw,
            bh,
        )
    }

    fn shown_level(&self) -> &Level {
        match &self.search {
            Some(g) => &g.current,
            None => &self.level,
        }
    }

    fn draw_board(&mut self) {
        let lv = self.shown_level().clone();
        let br = self.board_rect();
        let cell = br.w / lv.w as f32;
        let t = get_time() as f32;
        let generating = self.search.is_some();

        // Frame
        round_rect(Rect::new(br.x - 10.0, br.y - 10.0, br.w + 20.0, br.h + 20.0), 12.0, Color::new(0.28, 0.36, 0.48, 1.0));
        round_rect(Rect::new(br.x - 6.0, br.y - 6.0, br.w + 12.0, br.h + 12.0), 9.0, Color::new(0.18, 0.24, 0.33, 1.0));

        let dead_overlay = !generating && self.state == PlayState::Playing && self.show_solution;
        for y in 0..lv.h {
            for x in 0..lv.w {
                let i = lv.idx(x, y);
                let r = Rect::new(br.x + x as f32 * cell, br.y + y as f32 * cell, cell, cell);
                draw_tile(&lv, i, r, t);
                if dead_overlay && self.an.reached[i] && self.an.to_goal[i] == UNREACHABLE && i != lv.goal {
                    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.95, 0.3, 0.3, 0.22));
                }
            }
        }

        // Goal
        let (gx, gy) = lv.xy(lv.goal);
        let gc = vec2(br.x + (gx as f32 + 0.5) * cell, br.y + (gy as f32 + 0.5) * cell);
        let pulse = 0.5 + 0.5 * (t * 3.0).sin();
        match lv.rule {
            GoalRule::MustStop => {
                draw_circle(gc.x, gc.y, cell * 0.42, Color::new(1.0, 0.82, 0.3, 0.25 + 0.15 * pulse));
                draw_circle_lines(gc.x, gc.y, cell * 0.38, cell * 0.07, theme::GOLD);
                draw_circle_lines(gc.x, gc.y, cell * 0.22, cell * 0.06, theme::GOLD);
                draw_circle(gc.x, gc.y, cell * 0.08, theme::GOLD);
            }
            GoalRule::PassThrough => {
                draw_circle(gc.x, gc.y, cell * 0.42, Color::new(1.0, 0.82, 0.3, 0.25 + 0.15 * pulse));
                draw_star(gc, cell * 0.36, cell * 0.16, theme::GOLD, t * 0.6);
            }
        }

        // Start marker
        let (sx, sy) = lv.xy(lv.start);
        let sr = Rect::new(br.x + sx as f32 * cell, br.y + sy as f32 * cell, cell, cell);
        draw_rectangle_lines(sr.x + cell * 0.12, sr.y + cell * 0.12, cell * 0.76, cell * 0.76, 2.0, Color::new(0.2, 0.45, 0.7, 0.6));

        if generating {
            draw_rectangle(br.x, br.y, br.w, br.h, Color::new(0.05, 0.08, 0.13, 0.35));
            return;
        }

        // Solution overlay
        if self.show_solution {
            let mut c = lv.start;
            let mut pts = vec![c];
            for d in &self.an.solution {
                c = lv.slide(c, *d).cell();
                pts.push(c);
            }
            let center = |i: usize| {
                let (x, y) = lv.xy(i);
                vec2(br.x + (x as f32 + 0.5) * cell, br.y + (y as f32 + 0.5) * cell)
            };
            for (k, w) in pts.windows(2).enumerate() {
                let a = center(w[0]);
                let b = center(w[1]);
                let col = Color::new(1.0, 0.85, 0.35, 0.85);
                draw_line(a.x, a.y, b.x, b.y, cell * 0.08, col);
                draw_circle(a.x, a.y, cell * 0.16, col);
                let label = format!("{}", k + 1);
                let fs = (cell * 0.28).max(10.0);
                let tw = text_width(&label, fs);
                text(&label, a.x - tw / 2.0, a.y + fs * 0.35, fs, Color::new(0.15, 0.1, 0.0, 1.0));
            }
        }

        // Trail
        let (px, py) = self.player_cell_f();
        let pc = vec2(br.x + (px + 0.5) * cell, br.y + (py + 0.5) * cell);
        if self.slide.is_some() {
            self.trail.push((pc, 1.0));
        }
        let dt = get_frame_time();
        for (p, a) in self.trail.iter_mut() {
            *a -= dt * 3.0;
            if *a > 0.0 {
                draw_circle(p.x, p.y, cell * 0.22 * *a, Color::new(0.85, 0.97, 1.0, 0.35 * *a));
            }
        }
        self.trail.retain(|(_, a)| *a > 0.0);

        // Player
        let sinking = self.state == PlayState::Sank;
        let won = self.state == PlayState::Won;
        let scale = if sinking { 0.55 + 0.1 * (t * 6.0).sin() } else if won { 1.0 + 0.08 * (t * 5.0).sin() } else { 1.0 };
        let bump = if self.flash > 0.0 {
            let (dx, dy) = self.facing.delta();
            vec2(dx as f32, dy as f32) * (self.flash * 12.0).sin() * cell * 0.08
        } else {
            Vec2::ZERO
        };
        draw_penguin(pc + bump, cell * 0.42 * scale, self.facing, sinking);

        // Hint arrow
        if self.show_hint && self.state == PlayState::Playing && self.slide.is_none()
            && let Some(d) = hint(&lv, &self.an, self.pos) {
                let (dx, dy) = d.delta();
                let v = vec2(dx as f32, dy as f32);
                let bob = 0.5 + 0.5 * (t * 6.0).sin();
                let tip = pc + v * cell * (0.95 + 0.12 * bob);
                let base = pc + v * cell * 0.6;
                let perp = vec2(-v.y, v.x) * cell * 0.22;
                draw_triangle(tip, base + perp, base - perp, theme::GOLD);
            }
    }

    fn draw_top_bar(&mut self) {
        let left = if self.show_panel { PANEL_W } else { 0.0 };
        if self.ui.button(Rect::new(left + 14.0, 14.0, 40.0, 34.0), if self.show_panel { "<" } else { ">" }, true, false) {
            self.show_panel = !self.show_panel;
        }
        let x = left + 66.0;
        if let Some(g) = &self.search {
            let msg = format!(
                "Annealing...  iter {}   T {:.3}   cost {:.3}   best {:.3}",
                g.iter,
                g.temperature(),
                g.current_cost.total,
                g.best_cost.total
            );
            text(&msg, x, 37.0, 21.0, theme::ACCENT);
            return;
        }
        let moves = self.undo.len();
        let status = match self.state {
            PlayState::Playing => format!("Moves {}   |   Par {}", moves, self.an.par),
            PlayState::Won => format!("Solved in {} moves (par {})", moves, self.an.par),
            PlayState::Sank => "Splash! You fell through the ice.".to_owned(),
        };
        let col = match self.state {
            PlayState::Won => theme::GOOD,
            PlayState::Sank => theme::BAD,
            _ => theme::TEXT,
        };
        text(&status, x, 37.0, 24.0, col);
        if self.state == PlayState::Playing
            && self.warn_dead_ends
            && self.slide.is_none()
            && self.an.to_goal.get(self.pos) == Some(&UNREACHABLE)
        {
            let w = text_width(&status, 24.0);
            text("   Dead end: no way to the goal from here. Undo (U) or restart (R).", x + w, 37.0, 19.0, theme::WARN);
        }
    }

    fn draw_overlay(&mut self) {
        if self.search.is_some() || self.state == PlayState::Playing {
            return;
        }
        let br = self.board_rect();
        let w = 380.0f32.min(br.w + 40.0);
        let h = 170.0;
        let r = Rect::new(br.x + (br.w - w) / 2.0, br.y + (br.h - h) / 2.0, w, h);
        round_rect(r, 14.0, Color::new(0.06, 0.09, 0.15, 1.0));
        let cx = r.x + r.w / 2.0;
        let centered = |s: &str, y: f32, size: f32, c: Color| {
            let tw = text_width(s, size);
            text(s, cx - tw / 2.0, y, size, c);
        };
        if self.state == PlayState::Won {
            let moves = self.undo.len() as u32;
            let par = self.an.par.max(1);
            let stars = if moves <= par && self.hints_used == 0 { 3 } else if moves as f64 <= par as f64 * 1.5 { 2 } else { 1 };
            centered("Solved!", r.y + 42.0, 34.0, theme::GOOD);
            for k in 0..3 {
                let c = vec2(cx + (k as f32 - 1.0) * 46.0, r.y + 78.0);
                let col = if k < stars { theme::GOLD } else { theme::TRACK };
                draw_star(c, 18.0, 8.0, col, 0.0);
            }
            let extra = if self.hints_used > 0 { format!(" | {} hint(s)", self.hints_used) } else { String::new() };
            centered(&format!("{} moves | par {}{}", moves, par, extra), r.y + 118.0, 19.0, theme::TEXT);
        } else {
            centered("Splash!", r.y + 46.0, 34.0, theme::BAD);
            centered("The ice cracked under you.", r.y + 80.0, 19.0, theme::TEXT);
        }
        let bw = (r.w - 48.0) / 2.0;
        let b1 = Rect::new(r.x + 16.0, r.y + h - 46.0, bw, 32.0);
        let b2 = Rect::new(r.x + 32.0 + bw, r.y + h - 46.0, bw, 32.0);
        if self.state == PlayState::Won {
            if self.ui.button(b1, "Replay (R)", true, false) {
                self.reset_play();
            }
            if self.ui.button(b2, "Next level (N)", true, true) {
                self.new_level();
            }
        } else {
            if self.ui.button(b1, "Undo (U)", true, true) {
                self.do_undo();
            }
            if self.ui.button(b2, "Restart (R)", true, false) {
                self.reset_play();
            }
        }
    }

    fn draw_footer(&self) {
        let left = if self.show_panel { PANEL_W } else { 0.0 };
        let s = "Arrows/WASD or swipe: slide | U undo | R restart | N new | H hint | J auto-step | P solution | 1-0 difficulty | Tab panel";
        text(s, left + 24.0, screen_height() - 16.0, 16.0, theme::DIM);
    }

    fn draw_panel(&mut self) {
        if !self.show_panel {
            return;
        }
        let h = screen_height();
        draw_rectangle(0.0, 0.0, PANEL_W, h, theme::PANEL);
        draw_line(PANEL_W, 0.0, PANEL_W, h, 1.0, theme::PANEL_EDGE);
        let x = 20.0;
        let w = PANEL_W - 40.0;
        let mut y = 20.0;
        text("ICE SLIDE", x, y + 24.0, 32.0, theme::ACCENT);
        text(&format!("seed {}", self.seed), x + w - text_width(&format!("seed {}", self.seed), 16.0), y + 22.0, 16.0, theme::DIM);
        y += 46.0;

        // Master difficulty.
        let mut d = self.difficulty;
        let label = format!("{:.1}  {}", d, difficulty_name(d));
        let r = self.ui.slider("difficulty", Rect::new(x, y, w, 40.0), "Difficulty", &mut d, 1.0, 10.0, 0.5, &label, true);
        if r.changed {
            self.set_difficulty(d);
        }
        if r.released {
            self.start_generation();
        }
        y += 48.0;

        let bw = (w - 10.0) / 2.0;
        let busy = self.search.is_some();
        if self.ui.button(Rect::new(x, y, bw, 34.0), "New level (N)", true, true) {
            self.new_level();
        }
        if self.ui.button(Rect::new(x + bw + 10.0, y, bw, 34.0), "Re-roll same seed", !busy, false) {
            self.start_generation();
        }
        y += 42.0;
        let bw3 = (w - 20.0) / 3.0;
        if self.ui.button(Rect::new(x, y, bw3, 30.0), "Undo", !busy && !self.undo.is_empty(), false) {
            self.do_undo();
        }
        if self.ui.button(Rect::new(x + bw3 + 10.0, y, bw3, 30.0), "Restart", !busy, false) {
            self.reset_play();
        }
        if self.ui.button(Rect::new(x + 2.0 * (bw3 + 10.0), y, bw3, 30.0), if self.show_hint { "Hide hint" } else { "Hint" }, !busy && self.state == PlayState::Playing, false) {
            self.toggle_hint();
        }
        y += 40.0;
        self.ui.toggle(Rect::new(x, y, w, 24.0), "Show solution (P)", &mut self.show_solution);
        y += 28.0;
        self.ui.toggle(Rect::new(x, y, w, 24.0), "Warn when soft-locked", &mut self.warn_dead_ends);
        y += 28.0;
        self.ui.toggle(Rect::new(x, y, w, 24.0), "Advanced generator settings", &mut self.advanced);
        y += 34.0;

        if self.advanced {
            y = self.draw_advanced(x, y, w);
        }

        // Metrics of the current map vs. the target zones.
        y += 4.0;
        draw_line(x, y, x + w, y, 1.0, theme::PANEL_EDGE);
        y += 22.0;
        text("This map", x, y, 20.0, theme::TEXT);
        if self.search.is_none() {
            let (msg, col) = if self.in_zone { ("in difficulty zone", theme::GOOD) } else { ("closest found", theme::WARN) };
            text(msg, x + w - text_width(msg, 16.0), y, 16.0, col);
        }
        y += 10.0;
        let (an, s) = match &self.search {
            Some(g) => (g.best_analysis.clone(), g.settings.clone()),
            None => (self.an.clone(), self.settings.clone()),
        };
        let rows: [(&str, f64, Option<Zone>, bool); 5] = [
            ("Par (min moves)", an.par as f64, Some(s.par), false),
            ("Dead-end spots", an.dead_end_ratio(), Some(s.dead_ends), true),
            ("Misleading moves", an.misdirection_ratio(), Some(s.misdirection), true),
            ("Hole hazard", an.hazard_ratio(), if s.holes_enabled() { Some(s.hazard) } else { None }, true),
            ("Estimated difficulty", an.difficulty(), None, false),
        ];
        for (name, v, zone, pct) in rows {
            y += 22.0;
            let ok = zone.map(|z| an.solvable && z.contains(v));
            let dot = match ok {
                Some(true) => theme::GOOD,
                Some(false) => theme::WARN,
                None => theme::ACCENT,
            };
            draw_circle(x + 5.0, y - 5.0, 4.0, dot);
            text(name, x + 16.0, y, 17.0, theme::TEXT);
            let fmt = |v: f64| if pct { format!("{:.0}%", v * 100.0) } else { format!("{:.0}", v) };
            let shown = if !an.solvable {
                "-".to_owned()
            } else if name.starts_with("Estimated") {
                format!("{:.1} / 10", v)
            } else {
                fmt(v)
            };
            let zt = zone.map(|z| format!("  [{}-{}]", fmt(z.lo()), fmt(z.hi()))).unwrap_or_default();
            let full = format!("{shown}{zt}");
            text(&full, x + w - text_width(&full, 16.0), y, 16.0, theme::DIM);
        }
        y += 18.0;
        text(
            &format!("{} spots reachable | {} route(s) | {} decisions", an.reachable, an.optimal_paths, an.decisions),
            x,
            y + 4.0,
            14.0,
            theme::DIM,
        );
        y += 18.0;

        // Annealing graph.
        let hist: Vec<(u32, f64, f64)> = match &self.search {
            Some(g) => g.history.clone(),
            None => self.history.clone(),
        };
        let gh = (h - y - 40.0).clamp(0.0, 130.0);
        if gh > 50.0 {
            y += 10.0;
            let gr = Rect::new(x, y + 8.0, w, gh - 20.0);
            text("Annealing cost (log)", x, y + 2.0, 15.0, theme::DIM);
            if self.search.is_none() {
                let info = format!("{} iters | {:.0} ms", self.gen_iters, self.gen_time * 1000.0);
                text(&info, x + w - text_width(&info, 15.0), y + 2.0, 15.0, theme::DIM);
            }
            draw_graph(gr, &hist);
        }
    }

    fn draw_advanced(&mut self, x: f32, mut y: f32, w: f32) -> f32 {
        let mut changed = false;
        let mut released = false;
        let s = &mut self.settings;

        macro_rules! slider {
            ($id:expr, $label:expr, $val:expr, $min:expr, $max:expr, $step:expr, $shown:expr, $en:expr) => {{
                let mut v = $val;
                let r = self.ui.slider($id, Rect::new(x, y, w, 36.0), $label, &mut v, $min, $max, $step, &$shown, $en);
                changed |= r.changed;
                released |= r.released;
                y += 40.0;
                v
            }};
        }

        let wv = slider!("w", "Width", s.w as f64, 5.0, 24.0, 1.0, format!("{}", s.w), true);
        s.w = wv as usize;
        let hv = slider!("h", "Height", s.h as f64, 5.0, 18.0, 1.0, format!("{}", s.h), true);
        s.h = hv as usize;
        let pv = slider!("par", "Target par", s.par.center, 2.0, 40.0, 1.0, format!("{:.0} +/- {:.0}", s.par.center, s.par.tol), true);
        s.par.center = pv;
        let dv = slider!("dead", "Dead-end spots", s.dead_ends.center, 0.0, 0.6, 0.02, format!("{:.0}%", s.dead_ends.center * 100.0), true);
        s.dead_ends.center = dv;
        let mv = slider!("mis", "Misleading moves", s.misdirection.center, 0.0, 0.7, 0.02, format!("{:.0}%", s.misdirection.center * 100.0), true);
        s.misdirection.center = mv;
        let holes = s.hole_density > 0.0;
        let hz = slider!("haz", "Hole hazard", s.hazard.center, 0.0, 0.4, 0.02, format!("{:.0}%", s.hazard.center * 100.0), holes);
        s.hazard.center = hz;
        s.hazard.tol = s.hazard.tol.max(0.03);
        let rk = slider!("rock", "Rock density", s.rock_density, 0.04, 0.4, 0.01, format!("{:.0}%", s.rock_density * 100.0), true);
        s.rock_density = rk;
        let fl = slider!("snow", "Snow (stops you)", s.floor_density, 0.0, 0.15, 0.01, format!("{:.0}%", s.floor_density * 100.0), true);
        s.floor_density = fl;
        let ho = slider!("hole", "Holes (sink you)", s.hole_density, 0.0, 0.1, 0.005, format!("{:.1}%", s.hole_density * 100.0), true);
        s.hole_density = ho;

        let mut must = s.rule == GoalRule::MustStop;
        if self.ui.toggle(Rect::new(x, y, w, 24.0), "Must stop on the goal", &mut must) {
            self.settings.rule = if must { GoalRule::MustStop } else { GoalRule::PassThrough };
            released = true;
        }
        y += 32.0;
        let _ = changed;
        if released {
            self.start_generation();
        }
        y
    }
}

fn difficulty_name(d: f64) -> &'static str {
    match d {
        d if d < 2.5 => "Gentle",
        d if d < 4.5 => "Easy",
        d if d < 6.5 => "Tricky",
        d if d < 8.5 => "Hard",
        _ => "Brutal",
    }
}

fn hash2(a: usize, b: u32) -> f32 {
    let mut h = (a as u32).wrapping_mul(2654435761) ^ b.wrapping_mul(40503);
    h ^= h >> 13;
    h = h.wrapping_mul(1274126177);
    h ^= h >> 16;
    (h & 0xffff) as f32 / 65535.0
}

fn draw_tile(lv: &Level, i: usize, r: Rect, t: f32) {
    let tile = if i == lv.start || i == lv.goal { Tile::Ice } else { lv.tiles[i] };
    let (x, y) = lv.xy(i);
    let checker = if (x + y) % 2 == 0 { 0.0 } else { 0.025 };
    match tile {
        Tile::Ice | Tile::Hole => {
            let c = Color::new(0.68 - checker, 0.87 - checker, 0.96 - checker, 1.0);
            draw_rectangle(r.x, r.y, r.w, r.h, c);
            // Glints
            let g = hash2(i, 7);
            if g > 0.55 {
                let ox = r.x + r.w * (0.2 + 0.5 * hash2(i, 3));
                let oy = r.y + r.h * (0.2 + 0.5 * hash2(i, 5));
                let l = r.w * 0.18;
                let a = 0.35 + 0.25 * (t * 1.3 + g * 10.0).sin();
                draw_line(ox, oy, ox + l, oy - l, 1.5, Color::new(1.0, 1.0, 1.0, a));
            }
            if tile == Tile::Hole {
                let c = vec2(r.x + r.w / 2.0, r.y + r.h / 2.0);
                draw_circle(c.x, c.y, r.w * 0.40, Color::new(0.35, 0.55, 0.7, 1.0));
                draw_circle(c.x, c.y, r.w * 0.33, Color::new(0.05, 0.13, 0.25, 1.0));
                let ripple = (t * 1.5 + hash2(i, 9) * 6.0).sin() * 0.04;
                draw_circle_lines(c.x, c.y, r.w * (0.2 + ripple), 1.5, Color::new(0.3, 0.5, 0.75, 0.6));
                // crack lines
                for k in 0..3 {
                    let ang = hash2(i, 20 + k) * std::f32::consts::TAU;
                    let d = vec2(ang.cos(), ang.sin());
                    let a = c + d * r.w * 0.38;
                    let b = c + d * r.w * 0.5;
                    draw_line(a.x, a.y, b.x, b.y, 1.5, Color::new(0.35, 0.55, 0.7, 1.0));
                }
            }
        }
        Tile::Floor => {
            draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.95 - checker, 0.96 - checker, 0.98 - checker, 1.0));
            for k in 0..5 {
                let px = r.x + r.w * (0.12 + 0.76 * hash2(i, 30 + k));
                let py = r.y + r.h * (0.12 + 0.76 * hash2(i, 40 + k));
                draw_circle(px, py, r.w * 0.04, Color::new(0.78, 0.83, 0.9, 1.0));
            }
        }
        Tile::Rock => {
            let c = Color::new(0.68 - checker, 0.87 - checker, 0.96 - checker, 1.0);
            draw_rectangle(r.x, r.y, r.w, r.h, c);
            let cx = r.x + r.w / 2.0;
            let cy = r.y + r.h / 2.0 + r.h * 0.04;
            draw_circle(cx, cy + r.h * 0.06, r.w * 0.44, Color::new(0.2, 0.3, 0.4, 0.35));
            draw_circle(cx, cy, r.w * 0.43, Color::new(0.36, 0.38, 0.44, 1.0));
            draw_circle(cx - r.w * 0.06, cy - r.h * 0.07, r.w * 0.33, Color::new(0.48, 0.5, 0.56, 1.0));
            draw_circle(cx - r.w * 0.14, cy - r.h * 0.16, r.w * 0.11, Color::new(0.62, 0.64, 0.7, 1.0));
            // snow cap
            draw_circle(cx - r.w * 0.02, cy - r.h * 0.26, r.w * 0.16, Color::new(0.95, 0.97, 1.0, 0.85));
        }
    }
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.0, Color::new(0.55, 0.75, 0.88, 0.35));
}

fn draw_penguin(c: Vec2, r: f32, facing: Dir, sinking: bool) {
    let (dx, dy) = facing.delta();
    let f = vec2(dx as f32, dy as f32);
    let ink = Color::new(0.05, 0.05, 0.08, 1.0);
    let beak = Color::new(1.0, 0.62, 0.15, 1.0);
    // Shadow and body.
    draw_ellipse(c.x, c.y + r * 0.8, r * 0.8, r * 0.25, 0.0, Color::new(0.1, 0.2, 0.3, 0.25));
    let body = if sinking { Color::new(0.15, 0.2, 0.3, 1.0) } else { Color::new(0.12, 0.14, 0.2, 1.0) };
    draw_circle(c.x, c.y, r, body);
    match facing {
        // Seen from behind: just a little tail.
        Dir::Up => {
            draw_circle(c.x, c.y + r * 0.55, r * 0.18, Color::new(0.22, 0.25, 0.32, 1.0));
        }
        // Facing the camera: belly, two eyes, beak pointing down.
        Dir::Down => {
            draw_circle(c.x, c.y + r * 0.12, r * 0.68, Color::new(0.97, 0.97, 0.99, 1.0));
            for s in [-1.0f32, 1.0] {
                let e = c + vec2(s * r * 0.27, -r * 0.2);
                draw_circle(e.x, e.y, r * 0.11, ink);
                draw_circle(e.x + r * 0.03, e.y - r * 0.03, r * 0.04, WHITE);
            }
            let b = c + vec2(0.0, r * 0.02);
            draw_triangle(b + vec2(-r * 0.15, 0.0), b + vec2(r * 0.15, 0.0), b + vec2(0.0, r * 0.2), beak);
        }
        // Side view: belly shifted forward, one eye, beak sticking out.
        Dir::Left | Dir::Right => {
            let belly = c + f * r * 0.18 + vec2(0.0, r * 0.1);
            draw_circle(belly.x, belly.y, r * 0.68, Color::new(0.97, 0.97, 0.99, 1.0));
            let e = c + f * r * 0.35 - vec2(0.0, r * 0.25);
            draw_circle(e.x, e.y, r * 0.11, ink);
            draw_circle(e.x + r * 0.03, e.y - r * 0.03, r * 0.04, WHITE);
            let base = c + f * r * 0.55 - vec2(0.0, r * 0.05);
            draw_triangle(c + f * r * 0.95 - vec2(0.0, r * 0.05), base + vec2(0.0, r * 0.12), base - vec2(0.0, r * 0.12), beak);
        }
    }
    // Scarf.
    draw_circle_lines(c.x, c.y, r * 0.98, r * 0.08, Color::new(0.9, 0.3, 0.35, 0.9));
}

fn draw_star(c: Vec2, outer: f32, inner: f32, col: Color, rot: f32) {
    let n = 5;
    let pts: Vec<Vec2> = (0..n * 2)
        .map(|k| {
            let a = rot - std::f32::consts::FRAC_PI_2 + k as f32 * std::f32::consts::PI / n as f32;
            let rad = if k % 2 == 0 { outer } else { inner };
            c + vec2(a.cos(), a.sin()) * rad
        })
        .collect();
    for k in 0..pts.len() {
        draw_triangle(c, pts[k], pts[(k + 1) % pts.len()], col);
    }
}

fn draw_graph(r: Rect, hist: &[(u32, f64, f64)]) {
    round_rect(r, 6.0, Color::new(0.06, 0.08, 0.13, 1.0));
    if hist.len() < 2 {
        return;
    }
    let max_i = hist.last().unwrap().0.max(1) as f32;
    let max_c = hist.iter().map(|h| h.1).fold(0.0f64, f64::max).max(0.1);
    let ly = |c: f64| {
        let f = ((1.0 + c).ln() / (1.0 + max_c).ln()) as f32;
        r.y + r.h - 6.0 - f * (r.h - 12.0)
    };
    let lx = |i: u32| r.x + 6.0 + (i as f32 / max_i) * (r.w - 12.0);
    // Down-sample to the graph width.
    let step = (hist.len() as f32 / r.w).ceil().max(1.0) as usize;
    let pts: Vec<_> = hist.iter().step_by(step).chain(hist.last()).collect();
    for w in pts.windows(2) {
        draw_line(lx(w[0].0), ly(w[0].1), lx(w[1].0), ly(w[1].1), 1.0, Color::new(0.38, 0.78, 0.98, 0.55));
        draw_line(lx(w[0].0), ly(w[0].2), lx(w[1].0), ly(w[1].2), 2.0, theme::GOLD);
    }
    draw_line(r.x + 6.0, ly(0.0), r.x + r.w - 6.0, ly(0.0), 1.0, Color::new(0.4, 0.86, 0.55, 0.4));
}

#[macroquad::main(window_conf)]
async fn main() {
    let mut g = Game::new();
    loop {
        clear_background(theme::BG);
        g.ui.begin();
        let dt = get_frame_time().min(0.05);
        g.flash = (g.flash - dt).max(0.0);

        g.update_generation();
        g.handle_keys();
        g.update_slide(dt);

        // Draw panel first so it captures clicks before the board swipe logic.
        g.draw_panel();
        g.draw_top_bar();
        g.draw_board();
        g.draw_overlay();
        g.draw_footer();

        // Swipe / drag on the board.
        if is_mouse_button_pressed(MouseButton::Left) && !g.ui.captured {
            g.swipe_from = Some(g.ui.mouse);
        }
        if is_mouse_button_released(MouseButton::Left)
            && let Some(from) = g.swipe_from.take() {
                let d = g.ui.mouse - from;
                if d.length() > 24.0 {
                    let dir = if d.x.abs() > d.y.abs() {
                        if d.x > 0.0 { Dir::Right } else { Dir::Left }
                    } else if d.y > 0.0 {
                        Dir::Down
                    } else {
                        Dir::Up
                    };
                    g.try_move(dir);
                }
            }
        g.ui.end();
        next_frame().await;
    }
}
