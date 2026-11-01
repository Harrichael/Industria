# Ice Slide

An ice-sliding puzzle game with procedurally generated levels. You're a penguin
on a frozen lake. Pick a direction and you slide until something stops you. Get
to the goal.

| Tile | Behaviour |
|------|-----------|
| Ice | You keep sliding. |
| Rock | You stop on the tile just before it. The map edge works the same way. |
| Snow | You stop *on* it. |
| Hole | You fall through the ice. Undo or restart. |
| Goal ◎ | You have to **stop** on it ("Must stop on the goal"). |
| Goal ★ | Sliding **over** it is enough (easy levels). |

## Running it

```bash
# Native (desktop window)
cargo run --release

# Browser (WebAssembly)
./build_web.sh
python3 -m http.server -d web 8000   # then open http://localhost:8000
```

A prebuilt `web/ice_slide.wasm` is checked in, so you can also serve `web/` as
is (for example from GitHub Pages).

Headless generator, which prints maps and their metrics:

```bash
cargo run --release --bin icegen -- 7 5      # difficulty 7, 5 seeds
```

### Controls

| Key | Action |
|-----|--------|
| Arrows / WASD / swipe | Slide |
| U, Z, Backspace | Undo |
| R | Restart |
| N | New level |
| H | Show a hint arrow |
| J | Auto-step (make the next optimal move) |
| P | Show the optimal solution and the dead-end cells |
| 1 … 0 | Pick difficulty 1–10 and generate a new level |
| Space | Skip the annealing animation |
| Tab | Show or hide the side panel |

## How the generation works

### 1. Solver and metrics (`src/solver.rs`)

A state is just the cell the penguin is resting on. That means a whole map can
be solved exactly with a BFS over at most `w·h` states. From that one pass we get:

* **Par**: the minimum number of moves.
* **Dead-end ratio**: the share of reachable resting spots from which the goal
  can never be reached (you're soft-locked). A reverse BFS from the goal finds
  them.
* **Misdirection ratio**: the share of steps along the optimal line where the
  "obvious" move is wrong. The obvious move is the one that lands closest to the
  goal.
* **Hazard ratio**: the share of live moves that drop you into a hole.
* Decisions, number of optimal routes, and reachable-spot count.

These are combined into a 0–10 **estimated difficulty**. It's calibrated so a
map generated at difficulty *d* scores about *d*.

### 2. Simulated annealing (`src/generator.rs`)

Each slider defines a **target zone** `[center − tol, center + tol]` rather than
a single number. The cost of a map is 0 inside every zone and grows
quadratically outside. Unsolvable maps get a large cost, plus a term for the
gap between "cells I can reach" and "cells that can reach the goal", so the
search still has a direction to move in.

From a random map, the annealer repeatedly mutates a copy. A mutation can:

* toggle a rock
* nudge a rock one cell
* teleport a rock
* toggle snow or a hole
* move the start or the goal

It then re-solves and re-scores the copy. Better copies are always kept. Worse
ones are kept with probability `exp(−Δ/T)`, and `T` cools geometrically. When
progress stalls it jumps back to the best map and re-heats. If nothing solvable
has been found by then, it restarts from a fresh random map instead. It stops as
soon as a map is inside every zone, which usually takes 3k–12k iterations and
0.05–0.2 s natively. The side panel shows a live plot of the current cost (blue)
and best cost (gold).

### Difficulty controls

* **Difficulty** (master, 1–10): sets everything below to matching values.
* **Advanced** settings:
  * width and height
  * target par
  * dead-end %
  * misleading-move %
  * hole hazard %
  * densities of rocks, snow and holes
  * goal rule (stop on it, or pass over it)

  Changing any of them regenerates the map with the same seed.

## Layout

```
src/level.rs      tiles, sliding rules, ASCII parse/print
src/solver.rs     BFS solver + difficulty metrics + hints
src/generator.rs  settings/zones, cost function, mutations, annealer
src/rng.rs        small deterministic PRNG (same seed → same map everywhere)
src/main.rs       macroquad game: rendering, input, panel
src/ui.rs         tiny immediate-mode slider/button/toggle widgets
src/bin/icegen.rs headless generator / tuning tool
web/              HTML shell + JS loader for the WASM build
```

`cargo test --release` covers the slide rules and goal modes. It also
cross-checks the solver against brute force on hundreds of random maps,
checks that generation is deterministic, and checks that every difficulty level
reliably lands in its zone.
