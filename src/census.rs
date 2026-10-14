//! Finds the creatures living in a soup: still lifes, oscillators and spaceships.
//!
//! The live cells are split into separate objects, and each object is replayed alone
//! on an empty grid. If it comes back to its own shape within `max_period` generations
//! it is a real creature; where it comes back tells us whether it moved.

use std::collections::HashMap;

use serde::Serialize;

use crate::rule::Rule;
use crate::sim::Sim;

/// Objects larger than this are left uncatalogued.
const MAX_SIDE: usize = 16;
const MAX_CELLS: usize = 64;
/// Cells this close (Chebyshev distance) belong to the same object. At distance 3 two
/// cells share no neighbour, so separate objects can't affect each other's next step.
const LINK: usize = 2;

/// Live cells relative to the top-left of their bounding box, sorted.
pub type Shape = Vec<(u8, u8)>;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Spaceship,
    Oscillator,
    Still,
}

#[derive(Clone, Debug, Serialize)]
pub struct Species {
    pub kind: Kind,
    pub period: u32,
    /// Displacement per period. (0, 0) for still lifes and oscillators.
    pub dx: i32,
    pub dy: i32,
    pub speed: Option<String>,
    pub population: usize,
    pub width: usize,
    pub height: usize,
    /// One phase of the creature, as it was first seen.
    pub cells: Shape,
    /// How many times it turned up across the surveys.
    pub sightings: usize,
}

impl Species {
    /// Speed in cells per generation, for comparing spaceships.
    pub fn speed_value(&self) -> f64 {
        self.dx.abs().max(self.dy.abs()) as f64 / self.period as f64
    }
}

/// The usual notation: `c/4 diagonal`, `c/2 orthogonal`, `2c/5 orthogonal`, `(2,1)c/6 oblique`.
pub fn speed_label(dx: i32, dy: i32, period: u32) -> String {
    let (a, b) = (dx.abs().max(dy.abs()) as u32, dx.abs().min(dy.abs()) as u32);
    if b != 0 && a != b {
        return format!("({a},{b})c/{period} oblique");
    }
    let g = gcd(a, period);
    let (num, den) = (a / g, period / g);
    let fraction = match (num, den) {
        (1, 1) => "c".to_string(),
        (n, 1) => format!("{n}c"),
        (1, d) => format!("c/{d}"),
        (n, d) => format!("{n}c/{d}"),
    };
    format!("{fraction} {}", if b == 0 { "orthogonal" } else { "diagonal" })
}

fn gcd(a: u32, b: u32) -> u32 {
    if b == 0 { a.max(1) } else { gcd(b, a % b) }
}

/// Translates cells so the bounding box starts at (0, 0), and sorts them.
fn normalize(cells: impl Iterator<Item = (isize, isize)>) -> Shape {
    let cells: Vec<(isize, isize)> = cells.collect();
    let min_x = cells.iter().map(|c| c.0).min().unwrap_or(0);
    let min_y = cells.iter().map(|c| c.1).min().unwrap_or(0);
    let mut shape: Shape = cells.iter().map(|&(x, y)| ((x - min_x) as u8, (y - min_y) as u8)).collect();
    shape.sort_unstable();
    shape
}

fn extent(shape: &Shape) -> (usize, usize) {
    let w = shape.iter().map(|c| c.0 as usize + 1).max().unwrap_or(0);
    let h = shape.iter().map(|c| c.1 as usize + 1).max().unwrap_or(0);
    (w, h)
}

/// The same creature seen in any orientation or phase gets the same key.
fn canonical(phases: &[Shape]) -> Shape {
    let mut best: Option<Shape> = None;
    for phase in phases {
        for sym in 0..8 {
            let variant = normalize(phase.iter().map(|&(x, y)| {
                let (mut x, mut y) = (x as isize, y as isize);
                if sym & 4 != 0 {
                    (x, y) = (y, x);
                }
                if sym & 1 != 0 {
                    x = -x;
                }
                if sym & 2 != 0 {
                    y = -y;
                }
                (x, y)
            }));
            if best.as_ref().is_none_or(|b| variant < *b) {
                best = Some(variant);
            }
        }
    }
    best.unwrap_or_default()
}

/// Splits the live cells of a grid into separate objects. Objects near the edge are
/// skipped because on a torus they may continue on the other side.
pub fn objects(sim: &Sim) -> Vec<Shape> {
    let (w, h) = (sim.width, sim.height);
    let live: Vec<usize> = sim.cells().iter().enumerate().filter(|(_, c)| **c == 1).map(|(i, _)| i).collect();
    let mut slot = vec![u32::MAX; w * h];
    for (i, &c) in live.iter().enumerate() {
        slot[c] = i as u32;
    }
    let mut parent: Vec<u32> = (0..live.len() as u32).collect();
    fn find(parent: &mut [u32], mut i: u32) -> u32 {
        while parent[i as usize] != i {
            parent[i as usize] = parent[parent[i as usize] as usize];
            i = parent[i as usize];
        }
        i
    }
    let link = LINK as isize;
    for (i, &c) in live.iter().enumerate() {
        let (x, y) = ((c % w) as isize, (c / w) as isize);
        for dy in 0..=link {
            for dx in -link..=link {
                if dy == 0 && dx <= 0 {
                    continue;
                }
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || nx >= w as isize || ny >= h as isize {
                    continue;
                }
                let j = slot[ny as usize * w + nx as usize];
                if j != u32::MAX {
                    let (a, b) = (find(&mut parent, i as u32), find(&mut parent, j));
                    parent[a as usize] = b;
                }
            }
        }
    }
    let mut groups: HashMap<u32, Vec<(isize, isize)>> = HashMap::new();
    for (i, &c) in live.iter().enumerate() {
        let root = find(&mut parent, i as u32);
        groups.entry(root).or_default().push(((c % w) as isize, (c / w) as isize));
    }
    let margin = LINK as isize + 1;
    let mut shapes: Vec<Shape> = groups
        .into_values()
        .filter(|cells| cells.len() <= MAX_CELLS)
        .filter(|cells| {
            let (x0, x1) = (cells.iter().map(|c| c.0).min().unwrap(), cells.iter().map(|c| c.0).max().unwrap());
            let (y0, y1) = (cells.iter().map(|c| c.1).min().unwrap(), cells.iter().map(|c| c.1).max().unwrap());
            x1 - x0 < MAX_SIDE as isize
                && y1 - y0 < MAX_SIDE as isize
                && x0 >= margin
                && y0 >= margin
                && x1 < w as isize - margin
                && y1 < h as isize - margin
        })
        .map(|cells| normalize(cells.into_iter()))
        .collect();
    shapes.sort_unstable();
    shapes
}

struct Found {
    kind: Kind,
    period: u32,
    dx: i32,
    dy: i32,
    key: Shape,
}

/// Replays a shape alone. Returns what it is if it recurs within `max_period` generations.
fn classify(rule: Rule, shape: &Shape, max_period: u32) -> Option<Found> {
    let (w, h) = extent(shape);
    let margin = max_period as usize + 3;
    let (sw, sh) = (w + 2 * margin, h + 2 * margin);
    let mut sim = Sim::new(rule, sw, sh);
    for &(x, y) in shape {
        sim.set(x as usize + margin, y as usize + margin, true);
    }
    let mut phases = vec![shape.clone()];
    for t in 1..=max_period {
        sim.step();
        let live: Vec<(usize, usize)> = sim.live_cells().collect();
        if live.is_empty() || live.len() > 4 * MAX_CELLS {
            return None;
        }
        let (x0, x1) = (live.iter().map(|c| c.0).min().unwrap(), live.iter().map(|c| c.0).max().unwrap());
        let (y0, y1) = (live.iter().map(|c| c.1).min().unwrap(), live.iter().map(|c| c.1).max().unwrap());
        if x0 == 0 || y0 == 0 || x1 + 1 >= sw || y1 + 1 >= sh {
            return None; // grew or flew too far to be one small creature
        }
        let now = normalize(live.iter().map(|&(x, y)| (x as isize, y as isize)));
        if now == *shape {
            let (dx, dy) = (x0 as i32 - margin as i32, y0 as i32 - margin as i32);
            let kind = match (dx, dy, t) {
                (0, 0, 1) => Kind::Still,
                (0, 0, _) => Kind::Oscillator,
                _ => Kind::Spaceship,
            };
            return Some(Found { kind, period: t, dx, dy, key: canonical(&phases) });
        }
        phases.push(now);
    }
    None
}

/// The creatures found in one rule so far.
pub struct Census {
    rule: Rule,
    max_period: u32,
    /// Shapes already replayed, and the species key they turned out to be (None: not a creature).
    seen: HashMap<Shape, Option<Shape>>,
    species: HashMap<Shape, Species>,
}

impl Census {
    pub fn new(rule: Rule, max_period: u32) -> Census {
        Census { rule, max_period, seen: HashMap::new(), species: HashMap::new() }
    }

    /// Catalogues every separate object currently on the grid.
    pub fn survey(&mut self, sim: &Sim) {
        for shape in objects(sim) {
            self.record(shape);
        }
    }

    fn record(&mut self, shape: Shape) {
        let key = match self.seen.get(&shape) {
            Some(key) => key.clone(),
            None => {
                let found = classify(self.rule, &shape, self.max_period);
                let key = found.as_ref().map(|f| f.key.clone());
                if let Some(f) = found {
                    let (width, height) = extent(&shape);
                    self.species.entry(f.key.clone()).or_insert_with(|| Species {
                        kind: f.kind,
                        period: f.period,
                        dx: f.dx,
                        dy: f.dy,
                        speed: (f.kind == Kind::Spaceship).then(|| speed_label(f.dx, f.dy, f.period)),
                        population: shape.len(),
                        width,
                        height,
                        cells: shape.clone(),
                        sightings: 0,
                    });
                }
                self.seen.insert(shape, key.clone());
                key
            }
        };
        if let Some(key) = key {
            self.species.get_mut(&key).expect("recorded above").sightings += 1;
        }
    }

    /// Spaceships first, then oscillators, then still lifes; commonest first within each.
    pub fn species(&self) -> Vec<Species> {
        let mut all: Vec<Species> = self.species.values().cloned().collect();
        all.sort_by(|a, b| {
            (a.kind, std::cmp::Reverse(a.sightings), a.population, &a.cells).cmp(&(
                b.kind,
                std::cmp::Reverse(b.sightings),
                b.population,
                &b.cells,
            ))
        });
        all
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GLIDER: &[(usize, usize)] = &[(1, 0), (2, 1), (0, 2), (1, 2), (2, 2)];
    const LWSS: &[(usize, usize)] = &[(1, 0), (4, 0), (0, 1), (0, 2), (4, 2), (0, 3), (1, 3), (2, 3), (3, 3)];

    fn place(sim: &mut Sim, at: (usize, usize), cells: &[(usize, usize)], flip: bool) {
        for &(x, y) in cells {
            let x = if flip { 10 - x } else { x };
            sim.set(at.0 + x, at.1 + y, true);
        }
    }

    fn life_zoo() -> Sim {
        let mut sim = Sim::new(Rule::LIFE, 64, 64);
        place(&mut sim, (5, 5), GLIDER, false);
        place(&mut sim, (40, 5), GLIDER, true); // a mirror-image glider is the same species
        place(&mut sim, (20, 20), &[(0, 0), (1, 0), (2, 0)], false); // blinker
        place(&mut sim, (30, 30), &[(0, 0), (1, 0), (0, 1), (1, 1)], false); // block
        place(&mut sim, (10, 40), &[(1, 0), (2, 0), (0, 1), (3, 1), (1, 2), (2, 2)], false); // beehive
        place(&mut sim, (40, 40), LWSS, false);
        place(&mut sim, (0, 30), &[(0, 0), (1, 0), (0, 1), (1, 1)], false); // touches the edge: skipped
        sim
    }

    #[test]
    fn finds_the_classic_life_creatures() {
        let mut census = Census::new(Rule::LIFE, 16);
        census.survey(&life_zoo());
        let zoo = census.species();
        let labels: Vec<(Kind, u32, Option<&str>, usize)> =
            zoo.iter().map(|s| (s.kind, s.period, s.speed.as_deref(), s.sightings)).collect();
        assert_eq!(
            labels,
            vec![
                (Kind::Spaceship, 4, Some("c/4 diagonal"), 2),
                (Kind::Spaceship, 4, Some("c/2 orthogonal"), 1),
                (Kind::Oscillator, 2, None, 1),
                (Kind::Still, 1, None, 1),
                (Kind::Still, 1, None, 1),
            ]
        );
    }

    #[test]
    fn debris_is_not_a_creature() {
        // The R-pentomino keeps changing for over a thousand generations.
        let r: Shape = vec![(0, 1), (1, 0), (1, 1), (1, 2), (2, 0)];
        assert!(classify(Rule::LIFE, &r, 16).is_none());
    }

    #[test]
    fn speed_labels() {
        assert_eq!(speed_label(1, 1, 4), "c/4 diagonal");
        assert_eq!(speed_label(-2, 0, 4), "c/2 orthogonal");
        assert_eq!(speed_label(0, 3, 3), "c orthogonal");
        assert_eq!(speed_label(2, 0, 5), "2c/5 orthogonal");
        assert_eq!(speed_label(2, 1, 6), "(2,1)c/6 oblique");
    }

    #[test]
    fn canonical_key_ignores_orientation() {
        let g: Shape = normalize(GLIDER.iter().map(|&(x, y)| (x as isize, y as isize)));
        let rotated: Shape = normalize(GLIDER.iter().map(|&(x, y)| (-(y as isize), x as isize)));
        assert_ne!(g, rotated);
        assert_eq!(canonical(&[g]), canonical(&[rotated]));
    }
}
