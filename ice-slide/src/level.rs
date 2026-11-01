//! Level representation and the sliding-movement rules.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tile {
    /// Slippery: you keep sliding across it.
    Ice,
    /// Solid: you stop on the tile *before* it.
    Rock,
    /// Rough ground / snow: you stop *on* it.
    Floor,
    /// Crack in the ice: sliding onto it sinks you (restart / undo).
    Hole,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

impl Dir {
    pub const ALL: [Dir; 4] = [Dir::Up, Dir::Down, Dir::Left, Dir::Right];

    pub fn delta(self) -> (i32, i32) {
        match self {
            Dir::Up => (0, -1),
            Dir::Down => (0, 1),
            Dir::Left => (-1, 0),
            Dir::Right => (1, 0),
        }
    }

    pub fn arrow(self) -> char {
        match self {
            Dir::Up => '↑',
            Dir::Down => '↓',
            Dir::Left => '←',
            Dir::Right => '→',
        }
    }
}

/// How the goal tile behaves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoalRule {
    /// Sliding over the goal at any point finishes the level (it's a hole you
    /// drop into). Easier.
    PassThrough,
    /// You have to come to rest exactly on the goal. Harder.
    MustStop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Came to rest at this cell.
    Stopped(usize),
    /// Reached the goal.
    Won(usize),
    /// Fell into a hole at this cell.
    Sank(usize),
}

impl Outcome {
    pub fn cell(self) -> usize {
        match self {
            Outcome::Stopped(c) | Outcome::Won(c) | Outcome::Sank(c) => c,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Level {
    pub w: usize,
    pub h: usize,
    pub tiles: Vec<Tile>,
    pub start: usize,
    pub goal: usize,
    pub rule: GoalRule,
}

impl Level {
    pub fn new(w: usize, h: usize, rule: GoalRule) -> Self {
        assert!(w >= 2 && h >= 2);
        Level {
            w,
            h,
            tiles: vec![Tile::Ice; w * h],
            start: 0,
            goal: w * h - 1,
            rule,
        }
    }

    #[inline]
    pub fn idx(&self, x: usize, y: usize) -> usize {
        y * self.w + x
    }

    #[inline]
    pub fn xy(&self, i: usize) -> (usize, usize) {
        (i % self.w, i / self.w)
    }

    pub fn len(&self) -> usize {
        self.w * self.h
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[inline]
    pub fn step(&self, from: usize, d: Dir) -> Option<usize> {
        let (x, y) = self.xy(from);
        let (dx, dy) = d.delta();
        let nx = x as i32 + dx;
        let ny = y as i32 + dy;
        if nx < 0 || ny < 0 || nx >= self.w as i32 || ny >= self.h as i32 {
            None
        } else {
            Some(self.idx(nx as usize, ny as usize))
        }
    }

    /// Simulate a slide from `from` in direction `d`.
    ///
    /// The outer border of the map behaves like a wall. The start and goal
    /// cells are always treated as ice underneath.
    pub fn slide(&self, from: usize, d: Dir) -> Outcome {
        let mut cur = from;
        loop {
            let Some(next) = self.step(cur, d) else {
                return self.rest(cur);
            };
            match self.tiles[next] {
                Tile::Rock => return self.rest(cur),
                Tile::Hole => return Outcome::Sank(next),
                Tile::Floor => return self.rest(next),
                Tile::Ice => {
                    cur = next;
                    if cur == self.goal && self.rule == GoalRule::PassThrough {
                        return Outcome::Won(cur);
                    }
                }
            }
        }
    }

    /// Same as [`Level::slide`] but also returns every cell passed through
    /// (including the final one), for animation.
    pub fn slide_path(&self, from: usize, d: Dir) -> (Outcome, Vec<usize>) {
        let out = self.slide(from, d);
        let mut path = vec![from];
        let end = out.cell();
        let mut cur = from;
        while cur != end {
            match self.step(cur, d) {
                Some(n) => {
                    cur = n;
                    path.push(cur);
                }
                None => break,
            }
        }
        (out, path)
    }

    fn rest(&self, cell: usize) -> Outcome {
        if cell == self.goal {
            Outcome::Won(cell)
        } else {
            Outcome::Stopped(cell)
        }
    }

    pub fn count(&self, t: Tile) -> usize {
        self.tiles.iter().filter(|&&x| x == t).count()
    }

    /// Parse an ASCII map. `#` rock, `.` ice, `_` floor, `O` hole,
    /// `S` start, `G` goal. Mostly for tests.
    pub fn parse(s: &str, rule: GoalRule) -> Level {
        let rows: Vec<&str> = s
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect();
        let h = rows.len();
        let w = rows[0].chars().count();
        let mut lv = Level::new(w, h, rule);
        for (y, row) in rows.iter().enumerate() {
            assert_eq!(row.chars().count(), w, "ragged map");
            for (x, c) in row.chars().enumerate() {
                let i = lv.idx(x, y);
                lv.tiles[i] = match c {
                    '#' => Tile::Rock,
                    '_' => Tile::Floor,
                    'O' => Tile::Hole,
                    'S' => {
                        lv.start = i;
                        Tile::Ice
                    }
                    'G' => {
                        lv.goal = i;
                        Tile::Ice
                    }
                    _ => Tile::Ice,
                };
            }
        }
        lv
    }
}

impl fmt::Display for Level {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for y in 0..self.h {
            for x in 0..self.w {
                let i = self.idx(x, y);
                let c = if i == self.start {
                    'S'
                } else if i == self.goal {
                    'G'
                } else {
                    match self.tiles[i] {
                        Tile::Ice => '.',
                        Tile::Rock => '#',
                        Tile::Floor => '_',
                        Tile::Hole => 'O',
                    }
                };
                write!(f, "{c}")?;
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slides_until_rock_wall_or_floor() {
        let lv = Level::parse(
            "
            S...#.
            ......
            .._...
            ..O..G
            ",
            GoalRule::MustStop,
        );
        // Right: stops before the rock at x=4.
        assert_eq!(lv.slide(lv.start, Dir::Right), Outcome::Stopped(lv.idx(3, 0)));
        // Down from start: hits bottom wall.
        assert_eq!(lv.slide(lv.start, Dir::Down), Outcome::Stopped(lv.idx(0, 3)));
        // Up from (2,0) bottom... from (2,1) down: lands on floor at (2,2).
        assert_eq!(lv.slide(lv.idx(2, 0), Dir::Down), Outcome::Stopped(lv.idx(2, 2)));
        // From (0,3) right: sinks in hole at (2,3).
        assert_eq!(lv.slide(lv.idx(0, 3), Dir::Right), Outcome::Sank(lv.idx(2, 3)));
        // From (5,0) down: slides to bottom which is goal -> win.
        assert_eq!(lv.slide(lv.idx(5, 0), Dir::Down), Outcome::Won(lv.goal));
    }

    #[test]
    fn goal_rules_differ() {
        let map = "
            S.G.
            ....
        ";
        let stop = Level::parse(map, GoalRule::MustStop);
        let pass = Level::parse(map, GoalRule::PassThrough);
        assert_eq!(stop.slide(stop.start, Dir::Right), Outcome::Stopped(3));
        assert_eq!(pass.slide(pass.start, Dir::Right), Outcome::Won(2));
    }

    #[test]
    fn slide_path_lists_cells() {
        let lv = Level::parse("S..#\n....", GoalRule::MustStop);
        let (out, path) = lv.slide_path(0, Dir::Right);
        assert_eq!(out, Outcome::Stopped(2));
        assert_eq!(path, vec![0, 1, 2]);
    }

    #[test]
    fn display_roundtrip() {
        let map = "S..#\n._O.\n...G\n";
        let lv = Level::parse(map, GoalRule::MustStop);
        assert_eq!(lv.to_string(), map);
    }
}
