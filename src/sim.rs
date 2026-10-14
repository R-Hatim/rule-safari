//! A toroidal grid that steps any Life-like rule, and a small deterministic RNG.

use crate::rule::Rule;

/// SplitMix64. Small, fast, and good enough for seeding soups.
#[derive(Clone, Debug)]
pub struct Rng(u64);

fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

impl Rng {
    /// An independent stream for a tuple such as (seed, rule, soup number), so results
    /// never depend on which thread ran what.
    pub fn for_parts(parts: &[u64]) -> Rng {
        let mut h = 0x9e37_79b9_7f4a_7c15u64;
        for &p in parts {
            h = mix(h ^ p.wrapping_mul(0xd6e8_feb8_6659_fd93));
        }
        Rng(h)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        mix(self.0)
    }

    /// Uniform in [0, 1).
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in [0, n).
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }
}

/// A `width` x `height` torus of cells (0 or 1) and the rule that steps it.
#[derive(Clone)]
pub struct Sim {
    pub width: usize,
    pub height: usize,
    cells: Vec<u8>,
    next: Vec<u8>,
    cols: Vec<u8>,
    table: [u8; 18],
}

impl Sim {
    pub fn new(rule: Rule, width: usize, height: usize) -> Sim {
        assert!(width >= 3 && height >= 3, "the torus must be at least 3x3");
        Sim {
            width,
            height,
            cells: vec![0; width * height],
            next: vec![0; width * height],
            cols: vec![0; width],
            table: rule.table(),
        }
    }

    pub fn cells(&self) -> &[u8] {
        &self.cells
    }

    pub fn get(&self, x: usize, y: usize) -> bool {
        self.cells[y * self.width + x] == 1
    }

    pub fn set(&mut self, x: usize, y: usize, alive: bool) {
        self.cells[y * self.width + x] = alive as u8;
    }

    /// Fills the rectangle at (`x0`, `y0`) with random cells at the given density.
    pub fn fill_random(&mut self, x0: usize, y0: usize, w: usize, h: usize, density: f64, rng: &mut Rng) {
        for y in y0..y0 + h {
            for x in x0..x0 + w {
                let alive = rng.next_f64() < density;
                self.set(x % self.width, y % self.height, alive);
            }
        }
    }

    pub fn population(&self) -> usize {
        self.cells.iter().map(|&c| c as usize).sum()
    }

    /// Live cells as (x, y), row by row.
    pub fn live_cells(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        let w = self.width;
        self.cells.iter().enumerate().filter(|(_, c)| **c == 1).map(move |(i, _)| (i % w, i / w))
    }

    /// Advances one generation and returns how many cells changed.
    pub fn step(&mut self) -> usize {
        let (w, h) = (self.width, self.height);
        let (cells, next, cols, table) = (&self.cells, &mut self.next, &mut self.cols, &self.table);
        let mut changed = 0;
        for y in 0..h {
            let up = if y == 0 { h - 1 } else { y - 1 } * w;
            let down = if y + 1 == h { 0 } else { y + 1 } * w;
            let row = y * w;
            // Vertical sums of three, then each cell adds its column and both neighbours.
            for x in 0..w {
                cols[x] = cells[up + x] + cells[row + x] + cells[down + x];
            }
            for x in 0..w {
                let left = if x == 0 { w - 1 } else { x - 1 };
                let right = if x + 1 == w { 0 } else { x + 1 };
                let me = cells[row + x];
                let n = cols[left] + cols[x] + cols[right] - me;
                let new = table[(me * 9 + n) as usize];
                changed += (new != me) as usize;
                next[row + x] = new;
            }
        }
        std::mem::swap(&mut self.cells, &mut self.next);
        changed
    }

    /// A 64-bit fingerprint of the current state, for spotting cycles.
    pub fn fingerprint(&self) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        for chunk in self.cells.chunks(8) {
            let mut word = [0u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            h = (h ^ u64::from_le_bytes(word)).wrapping_mul(0x0000_0100_0000_01b3).rotate_left(29);
        }
        mix(h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(sim: &mut Sim, cells: &[(usize, usize)]) {
        for &(x, y) in cells {
            sim.set(x, y, true);
        }
    }

    fn live(sim: &Sim) -> Vec<(usize, usize)> {
        sim.live_cells().collect()
    }

    #[test]
    fn blinker_oscillates_with_period_two() {
        let mut sim = Sim::new(Rule::LIFE, 8, 8);
        place(&mut sim, &[(2, 3), (3, 3), (4, 3)]);
        let start = live(&sim);
        assert_eq!(sim.step(), 4, "two cells die, two are born");
        assert_eq!(live(&sim), vec![(3, 2), (3, 3), (3, 4)]);
        sim.step();
        assert_eq!(live(&sim), start);
    }

    #[test]
    fn block_is_still() {
        let mut sim = Sim::new(Rule::LIFE, 6, 6);
        place(&mut sim, &[(2, 2), (3, 2), (2, 3), (3, 3)]);
        assert_eq!(sim.step(), 0);
        assert_eq!(sim.population(), 4);
    }

    #[test]
    fn glider_wraps_around_the_torus() {
        let mut sim = Sim::new(Rule::LIFE, 10, 10);
        place(&mut sim, &[(1, 0), (2, 1), (0, 2), (1, 2), (2, 2)]);
        let start = sim.fingerprint();
        // A glider moves one cell diagonally every 4 generations: 40 to come home.
        for _ in 0..40 {
            sim.step();
            assert_eq!(sim.population(), 5);
        }
        assert_eq!(sim.fingerprint(), start);
    }

    #[test]
    fn rng_streams_are_reproducible_and_distinct() {
        let draw = |parts: &[u64]| {
            let mut r = Rng::for_parts(parts);
            (0..4).map(|_| r.next_u64()).collect::<Vec<_>>()
        };
        assert_eq!(draw(&[1, 2]), draw(&[1, 2]));
        assert_ne!(draw(&[1, 2]), draw(&[2, 1]));
        let mut r = Rng::for_parts(&[7]);
        assert!((0..1000).map(|_| r.next_f64()).all(|f| (0.0..1.0).contains(&f)));
    }
}
