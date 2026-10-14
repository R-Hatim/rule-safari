//! Measures of how a soup behaves: Lempel-Ziv structure, and a cycle detector.

use std::collections::VecDeque;

use crate::sim::Rng;

/// Number of phrases in the LZ78 parse of a bit sequence. Repetitive input parses into
/// few long phrases; noise parses into many short ones.
pub fn lz78_phrases(bits: &[u8]) -> usize {
    let mut trie: Vec<[u32; 2]> = vec![[0, 0]];
    let (mut node, mut phrases) = (0usize, 0usize);
    for &b in bits {
        let b = (b & 1) as usize;
        match trie[node][b] {
            0 => {
                trie.push([0, 0]);
                trie[node][b] = (trie.len() - 1) as u32;
                phrases += 1;
                node = 0;
            }
            child => node = child as usize,
        }
    }
    phrases + (node != 0) as usize
}

/// How patterned some cells are compared with noise of the same density: 0 when they
/// compress no better than the same cells shuffled, rising towards 1 for strong regularity.
/// Comparing against a shuffle cancels LZ78's bias on short inputs and at low densities.
pub fn structure(bits: &[u8], rng: &mut Rng) -> f64 {
    let ones = bits.iter().filter(|&&b| b == 1).count();
    if ones == 0 || ones == bits.len() {
        return 1.0;
    }
    let mut shuffled = bits.to_vec();
    for i in (1..shuffled.len()).rev() {
        shuffled.swap(i, rng.below(i + 1));
    }
    let actual = lz78_phrases(bits) as f64;
    let noise = lz78_phrases(&shuffled) as f64;
    (1.0 - actual / noise).clamp(0.0, 1.0)
}

/// Remembers recent state fingerprints to notice when a soup starts repeating itself.
pub struct CycleDetector {
    recent: VecDeque<u64>,
    window: usize,
}

impl CycleDetector {
    pub fn new(window: usize) -> CycleDetector {
        CycleDetector { recent: VecDeque::with_capacity(window + 1), window }
    }

    /// Records a fingerprint; returns the cycle period if it was seen in the last `window` steps.
    pub fn observe(&mut self, fingerprint: u64) -> Option<usize> {
        let period = self.recent.iter().rev().position(|&h| h == fingerprint).map(|i| i + 1);
        self.recent.push_back(fingerprint);
        if self.recent.len() > self.window {
            self.recent.pop_front();
        }
        period
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(n: usize, density: f64, seed: u64) -> Vec<u8> {
        let mut rng = Rng::for_parts(&[seed]);
        (0..n).map(|_| (rng.next_f64() < density) as u8).collect()
    }

    #[test]
    fn repetition_parses_into_fewer_phrases_than_noise() {
        // LZ78 needs length to shine: a periodic input grows like sqrt(n) phrases, noise like n/log n.
        let stripes: Vec<u8> = (0..1 << 16).map(|i| (i / 3 % 2) as u8).collect();
        assert!(lz78_phrases(&stripes) * 3 < lz78_phrases(&noise(1 << 16, 0.5, 1)));
        assert_eq!(lz78_phrases(&[]), 0);
        assert_eq!(lz78_phrases(&[1, 0, 1, 1]), 3, "1 | 0 | 11");
    }

    #[test]
    fn structure_separates_pattern_from_noise() {
        let mut rng = Rng::for_parts(&[9]);
        let stripes: Vec<u8> = (0..8192).map(|i| (i % 7 < 2) as u8).collect();
        assert!(structure(&stripes, &mut rng) > 0.6);
        for density in [0.05, 0.3, 0.5] {
            let s = structure(&noise(8192, density, 3), &mut rng);
            assert!(s < 0.08, "noise at {density} scored {s}");
        }
        assert_eq!(structure(&[0; 100], &mut rng), 1.0);
    }

    #[test]
    fn cycle_detector_reports_the_period() {
        let mut d = CycleDetector::new(8);
        let seq = [1, 2, 3, 4, 2, 3, 4];
        let found: Vec<Option<usize>> = seq.iter().map(|&h| d.observe(h)).collect();
        assert_eq!(found, vec![None, None, None, None, Some(3), Some(3), Some(3)]);
    }

    #[test]
    fn cycle_detector_forgets_beyond_its_window() {
        let mut d = CycleDetector::new(2);
        d.observe(1);
        d.observe(2);
        d.observe(3);
        assert_eq!(d.observe(1), None);
    }
}
