//! A tiny immediate-mode UI (sliders, buttons, toggles) drawn with macroquad
//! primitives. macroquad ships its own UI, but drawing our own gives us a look
//! that matches the game and works the same with mouse and touch.

use macroquad::prelude::*;

pub mod theme {
    use macroquad::prelude::Color;
    pub const BG: Color = Color::new(0.055, 0.075, 0.12, 1.0);
    pub const PANEL: Color = Color::new(0.085, 0.11, 0.17, 1.0);
    pub const PANEL_EDGE: Color = Color::new(0.16, 0.2, 0.29, 1.0);
    pub const TEXT: Color = Color::new(0.88, 0.92, 0.97, 1.0);
    pub const DIM: Color = Color::new(0.55, 0.62, 0.72, 1.0);
    pub const ACCENT: Color = Color::new(0.38, 0.78, 0.98, 1.0);
    pub const ACCENT_DARK: Color = Color::new(0.16, 0.36, 0.52, 1.0);
    pub const TRACK: Color = Color::new(0.2, 0.25, 0.34, 1.0);
    pub const GOOD: Color = Color::new(0.4, 0.86, 0.55, 1.0);
    pub const WARN: Color = Color::new(0.98, 0.66, 0.3, 1.0);
    pub const BAD: Color = Color::new(0.95, 0.38, 0.4, 1.0);
    pub const GOLD: Color = Color::new(1.0, 0.82, 0.3, 1.0);
}

#[derive(Default)]
pub struct Ui {
    pub mouse: Vec2,
    pressed: bool,
    down: bool,
    released: bool,
    active: Option<&'static str>,
    /// True when the current press started on a widget (so the board ignores it).
    pub captured: bool,
}

#[derive(Default, Clone, Copy)]
pub struct SliderResponse {
    pub changed: bool,
    pub released: bool,
}

pub fn text(s: &str, x: f32, y: f32, size: f32, c: Color) {
    draw_text(s, x.round(), y.round(), size, c);
}

pub fn text_width(s: &str, size: f32) -> f32 {
    measure_text(s, None, size as u16, 1.0).width
}

pub fn round_rect(r: Rect, rad: f32, c: Color) {
    let rad = rad.min(r.w / 2.0).min(r.h / 2.0);
    draw_rectangle(r.x + rad, r.y, r.w - 2.0 * rad, r.h, c);
    draw_rectangle(r.x, r.y + rad, rad, r.h - 2.0 * rad, c);
    draw_rectangle(r.x + r.w - rad, r.y + rad, rad, r.h - 2.0 * rad, c);
    draw_circle(r.x + rad, r.y + rad, rad, c);
    draw_circle(r.x + r.w - rad, r.y + rad, rad, c);
    draw_circle(r.x + rad, r.y + r.h - rad, rad, c);
    draw_circle(r.x + r.w - rad, r.y + r.h - rad, rad, c);
}

impl Ui {
    pub fn begin(&mut self) {
        let (x, y) = mouse_position();
        self.mouse = vec2(x, y);
        self.pressed = is_mouse_button_pressed(MouseButton::Left);
        self.down = is_mouse_button_down(MouseButton::Left);
        self.released = is_mouse_button_released(MouseButton::Left);
        if self.pressed {
            self.captured = false;
        }
    }

    pub fn end(&mut self) {
        if !self.down && !self.pressed {
            self.active = None;
        }
    }

    pub fn press_in(&mut self, r: Rect) -> bool {
        if self.pressed && r.contains(self.mouse) {
            self.captured = true;
            true
        } else {
            false
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn slider(
        &mut self,
        id: &'static str,
        r: Rect,
        label: &str,
        value: &mut f64,
        min: f64,
        max: f64,
        step: f64,
        shown: &str,
        enabled: bool,
    ) -> SliderResponse {
        let mut resp = SliderResponse::default();
        let track = Rect::new(r.x, r.y + 22.0, r.w, 6.0);
        let hit = Rect::new(r.x - 6.0, r.y + 10.0, r.w + 12.0, 28.0);
        if enabled && self.press_in(hit) {
            self.active = Some(id);
        }
        let active = self.active == Some(id);
        if active && (self.down || self.pressed) {
            let f = ((self.mouse.x - track.x) / track.w).clamp(0.0, 1.0) as f64;
            let mut v = min + f * (max - min);
            v = (v / step).round() * step;
            v = v.clamp(min, max);
            if (v - *value).abs() > 1e-9 {
                *value = v;
                resp.changed = true;
            }
        }
        if active && self.released {
            resp.released = true;
        }

        let hover = enabled && hit.contains(self.mouse);
        let lc = if enabled { theme::TEXT } else { theme::DIM };
        text(label, r.x, r.y + 13.0, 17.0, lc);
        let vw = text_width(shown, 17.0);
        text(shown, r.x + r.w - vw, r.y + 13.0, 17.0, if enabled { theme::ACCENT } else { theme::DIM });
        round_rect(track, 3.0, theme::TRACK);
        let f = ((*value - min) / (max - min)).clamp(0.0, 1.0) as f32;
        let fill = Rect::new(track.x, track.y, track.w * f, track.h);
        if fill.w > 1.0 {
            round_rect(fill, 3.0, if enabled { theme::ACCENT_DARK } else { theme::TRACK });
        }
        let kx = track.x + track.w * f;
        let kr = if active { 9.0 } else if hover { 8.0 } else { 7.0 };
        draw_circle(kx, track.y + 3.0, kr, if enabled { theme::ACCENT } else { theme::DIM });
        resp
    }

    pub fn button(&mut self, r: Rect, label: &str, enabled: bool, primary: bool) -> bool {
        let hover = enabled && r.contains(self.mouse);
        let clicked = enabled && self.press_in(r);
        let base = if primary { theme::ACCENT_DARK } else { theme::TRACK };
        let c = if !enabled {
            // Opaque (overlapping primitives would show seams with alpha).
            Color::new(
                base.r * 0.5 + theme::PANEL.r * 0.5,
                base.g * 0.5 + theme::PANEL.g * 0.5,
                base.b * 0.5 + theme::PANEL.b * 0.5,
                1.0,
            )
        } else if hover {
            Color::new(base.r * 1.25, base.g * 1.25, base.b * 1.25, 1.0)
        } else {
            base
        };
        round_rect(r, 7.0, c);
        let tw = text_width(label, 17.0);
        text(
            label,
            r.x + (r.w - tw) / 2.0,
            r.y + r.h / 2.0 + 5.0,
            17.0,
            if enabled { theme::TEXT } else { theme::DIM },
        );
        clicked
    }

    pub fn toggle(&mut self, r: Rect, label: &str, value: &mut bool) -> bool {
        let clicked = self.press_in(r);
        if clicked {
            *value = !*value;
        }
        let sw = Rect::new(r.x + r.w - 38.0, r.y + 2.0, 38.0, 20.0);
        round_rect(sw, 10.0, if *value { theme::ACCENT_DARK } else { theme::TRACK });
        let kx = if *value { sw.x + sw.w - 10.0 } else { sw.x + 10.0 };
        draw_circle(kx, sw.y + 10.0, 7.5, if *value { theme::ACCENT } else { theme::DIM });
        text(label, r.x, r.y + 17.0, 17.0, theme::TEXT);
        clicked
    }
}
