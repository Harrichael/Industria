//! Procedurally generated ice-sliding puzzles.
//!
//! * [`level`] – the map and the sliding rules.
//! * [`solver`] – exhaustive BFS solver that also measures how *hard* a map is.
//! * [`generator`] – simulated-annealing generator that mutates maps until
//!   their measured difficulty lands inside a requested zone.

pub mod generator;
pub mod level;
pub mod rng;
pub mod solver;
