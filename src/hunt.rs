//! The hunt: screen every rule on a small soup, study the promising ones closely, rank them.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use rayon::prelude::*;
use serde::Serialize;

use crate::census::{Census, Kind, Species};
use crate::metrics::{CycleDetector, structure};
use crate::rule::{RULES_WITHOUT_B0, Rule, named_rules};
use crate::sim::{Rng, Sim};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    pub seed: u64,
    /// Fraction of live cells in a random soup.
    pub density: f64,
    /// The screen keeps the best of this many small soups, so one unlucky soup can't sink a rule.
    pub screen_soups: u32,
    pub screen_size: usize,
    pub screen_steps: u32,
    /// Tiny nurseries in the screen, so rules with spaceships get noticed early.
    pub screen_nurseries: u32,
    pub screen_nursery_size: usize,
    pub screen_nursery_steps: u32,
    /// How many of the best-screened rules get a close look.
    pub finalists: usize,
    pub soup_size: usize,
    pub soup_steps: u32,
    pub soups: u32,
    /// Small soups dropped in an empty field, where creatures can escape the crowd.
    pub nurseries: u32,
    pub nursery_size: usize,
    pub nursery_seed: usize,
    pub nursery_steps: u32,
    pub max_period: u32,
    /// Screen a random sample of this many rules instead of all of them.
    pub sample: Option<usize>,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            seed: 1,
            density: 0.375,
            screen_soups: 2,
            screen_size: 40,
            screen_steps: 160,
            screen_nurseries: 2,
            screen_nursery_size: 32,
            screen_nursery_steps: 96,
            finalists: 1500,
            soup_size: 128,
            soup_steps: 600,
            soups: 2,
            nurseries: 32,
            nursery_size: 64,
            nursery_seed: 8,
            nursery_steps: 240,
            max_period: 16,
            sample: None,
        }
    }
}

impl Config {
    /// A few seconds instead of a minute: a sample of rules and smaller soups.
    pub fn quick() -> Config {
        Config { sample: Some(4096), finalists: 96, soup_size: 96, soup_steps: 400, nurseries: 16, ..Config::default() }
    }
}

/// What one soup did.
#[derive(Clone, Copy, Debug)]
pub struct SoupRun {
    pub died: bool,
    /// Generations before the whole grid fell into a cycle; `steps` if it never did.
    pub lifespan: u32,
    pub settled: bool,
    /// Mean fraction of cells changing per generation over the last quarter of the run.
    pub activity: f64,
    /// Fraction of cells that differ from `MOTION_LAG` generations earlier, late in the run.
    /// Oscillators of period 1, 2, 3, 4, 6 and 12 cancel out, so this is change that goes
    /// somewhere: spaceships, growing debris, or noise.
    pub motion: f64,
    pub density: f64,
    pub structure: f64,
}

const MOTION_LAG: u32 = 12;
/// Nurseries are surveyed this often, as well as at the end.
const SURVEY_EVERY: u32 = 24;
/// How many lagged comparisons the motion average keeps (the latest ones).
const MOTION_SAMPLES: usize = 8;

/// Runs a random soup on a torus, stopping early once it repeats itself.
pub fn run_soup(rule: Rule, size: usize, steps: u32, density: f64, rng: &mut Rng) -> (SoupRun, Sim) {
    let mut sim = Sim::new(rule, size, size);
    sim.fill_random(0, 0, size, size, density, rng);
    let cells = (size * size) as f64;
    let mut cycles = CycleDetector::new(64);
    let tail_from = steps - steps / 4;
    let snapshots_from = steps.saturating_sub(6);
    let (mut tail_changes, mut tail_steps) = (0usize, 0u32);
    let mut frames: Vec<u8> = Vec::new();
    let mut lagged: Vec<u8> = sim.cells().to_vec();
    let mut motions: Vec<f64> = Vec::new();
    let mut lifespan = steps;
    let mut settled = false;
    for t in 1..=steps {
        let changed = sim.step();
        if t > tail_from {
            tail_changes += changed;
            tail_steps += 1;
        }
        if t % MOTION_LAG == 0 {
            let moved = lagged.iter().zip(sim.cells()).filter(|(a, b)| a != b).count();
            motions.push(moved as f64 / cells);
            lagged.copy_from_slice(sim.cells());
        }
        if t >= snapshots_from && (steps - t).is_multiple_of(2) {
            frames.extend_from_slice(sim.cells());
        }
        if cycles.observe(sim.fingerprint()).is_some() {
            lifespan = t;
            settled = true;
            break;
        }
    }
    if settled {
        frames = sim.cells().to_vec();
    }
    let recent = &motions[motions.len().saturating_sub(MOTION_SAMPLES)..];
    let population = sim.population();
    let run = SoupRun {
        died: population == 0,
        lifespan,
        settled,
        activity: if settled || tail_steps == 0 { 0.0 } else { tail_changes as f64 / tail_steps as f64 / cells },
        motion: if recent.is_empty() { 0.0 } else { recent.iter().sum::<f64>() / recent.len() as f64 },
        density: population as f64 / cells,
        structure: structure(&frames, rng),
    };
    (run, sim)
}

/// How lively a soup's motion is: 0 when it is frozen or only flickers in place, 1 across
/// the wide band where things travel and evolve, fading as it turns into noise.
fn liveliness(motion: f64) -> f64 {
    let x = motion.max(1e-6).log10();
    let ramp = |from: f64, to: f64| ((x - from) / (to - from)).clamp(0.0, 1.0);
    let rise = ramp(-3.0, -2.3); // 0.1% .. 0.5%
    let fall = 1.0 - 0.9 * ramp(-1.0, -0.456); // 10% .. 35%
    rise.min(fall)
}

/// Interesting things happen against a quiet background: crowded soups count for less.
fn quietness(density: f64) -> f64 {
    1.0 - 0.5 * ((density - 0.2) / 0.4).clamp(0.0, 1.0)
}

/// Soups that stay lively for the whole run, without filling up, score high.
fn dynamics(lifespan: f64, motion: f64, density: f64, extinct: f64) -> f64 {
    lifespan * liveliness(motion) * quietness(density) * (1.0 - extinct)
}

/// Spaceships count most, then oscillators (more beyond period 2), and still lifes a little.
fn zoo_richness(zoo: &[Species]) -> f64 {
    let ships = zoo.iter().filter(|s| s.kind == Kind::Spaceship).count() as f64;
    let oscillators: f64 =
        zoo.iter().filter(|s| s.kind == Kind::Oscillator).map(|s| if s.period > 2 { 1.5 } else { 0.5 }).sum();
    let stills = zoo.iter().filter(|s| s.kind == Kind::Still).count() as f64;
    let ship_part = ((1.0 + ships).ln() / 7f64.ln()).min(1.0);
    let rest = ((1.0 + oscillators + 0.1 * stills).ln() / 31f64.ln()).min(1.0);
    0.6 * ship_part + 0.4 * rest
}

fn score(dynamics: f64, zoo: f64) -> f64 {
    100.0 * (0.55 * dynamics + 0.45 * zoo)
}

/// Drops a small random seed into an empty field, lets it run, and surveys what comes out.
fn nursery(rule: Rule, census: &mut Census, field: usize, seed: usize, steps: u32, rng: &mut Rng) {
    let mut sim = Sim::new(rule, field, field);
    let at = (field - seed) / 2;
    sim.fill_random(at, at, seed, seed, 0.5, rng);
    let mut cycles = CycleDetector::new(64);
    for t in 1..=steps {
        sim.step();
        // Look often: spaceships tend to crash into the debris once they wrap around.
        if t % SURVEY_EVERY == 0 {
            census.survey(&sim);
        }
        if cycles.observe(sim.fingerprint()).is_some() {
            break;
        }
    }
    census.survey(&sim);
}

/// The first, cheap pass: a couple of small soups and tiny nurseries per rule.
pub fn screen(rule: Rule, cfg: &Config) -> f64 {
    let mut rng = Rng::for_parts(&[cfg.seed, rule.index() as u64, 0x5c4ee7]);
    let mut census = Census::new(rule, cfg.max_period);
    let mut best = 0.0f64;
    for _ in 0..cfg.screen_soups {
        let (run, sim) = run_soup(rule, cfg.screen_size, cfg.screen_steps, cfg.density, &mut rng);
        census.survey(&sim);
        let lifespan = run.lifespan as f64 / cfg.screen_steps as f64;
        best = best.max(dynamics(lifespan, run.motion, run.density, run.died as u8 as f64));
    }
    for n in 0..cfg.screen_nurseries {
        let seed = if n % 2 == 0 { 6 } else { 4 };
        nursery(rule, &mut census, cfg.screen_nursery_size, seed, cfg.screen_nursery_steps, &mut rng);
    }
    score(best, zoo_richness(&census.species()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Temperament {
    /// Soups die out.
    Extinct,
    /// Soups lock into a still or cycling state almost at once.
    Frozen,
    /// Soups stay busy for a while, then settle.
    SlowBurn,
    /// Soups never settle, but only flicker in place.
    Simmering,
    /// Soups stay lively and clump into large blobs.
    Coagulating,
    /// Soups stay dense, stirring everywhere at once.
    Teeming,
    /// Soups stay lively, with small things moving through sparse space.
    Complex,
    /// Soups never settle, and look like noise.
    Chaotic,
}

/// Everything learned about one rule.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Specimen {
    pub rule: String,
    pub name: Option<&'static str>,
    pub score: f64,
    pub temperament: Temperament,
    /// Mean fraction of the run before soups settled (1 = never settled).
    pub lifespan: f64,
    pub activity: f64,
    pub motion: f64,
    pub density: f64,
    pub structure: f64,
    /// Fraction of soups that died out.
    pub extinct: f64,
    pub ships: usize,
    pub oscillators: usize,
    pub still_lifes: usize,
    pub zoo: Vec<Species>,
    pub screen_score: f64,
    /// Position in the screen (1 = best), when the rule was screened.
    pub screen_rank: Option<usize>,
    #[serde(skip)]
    pub value: Rule,
}

/// The close look: big soups for behaviour, nurseries for the census.
pub fn evaluate(rule: Rule, cfg: &Config) -> Specimen {
    let mut census = Census::new(rule, cfg.max_period);
    let mut runs = Vec::new();
    for soup in 0..cfg.soups {
        let mut rng = Rng::for_parts(&[cfg.seed, rule.index() as u64, 0x50f9, soup as u64]);
        let (run, sim) = run_soup(rule, cfg.soup_size, cfg.soup_steps, cfg.density, &mut rng);
        census.survey(&sim);
        runs.push(run);
    }
    for n in 0..cfg.nurseries {
        let mut rng = Rng::for_parts(&[cfg.seed, rule.index() as u64, 0x4e55, n as u64]);
        // Alternate big and small seeds: explosive rules only release creatures from small ones.
        let seed = if n % 2 == 0 { cfg.nursery_seed } else { cfg.nursery_seed / 2 + 1 };
        nursery(rule, &mut census, cfg.nursery_size, seed, cfg.nursery_steps, &mut rng);
    }

    let n = runs.len().max(1) as f64;
    let mean = |f: &dyn Fn(&SoupRun) -> f64| runs.iter().map(f).sum::<f64>() / n;
    let lifespan = mean(&|r| r.lifespan as f64 / cfg.soup_steps as f64);
    let extinct = mean(&|r| r.died as u8 as f64);
    let activity = mean(&|r| r.activity);
    let motion = mean(&|r| r.motion);
    let density = mean(&|r| r.density);
    let structure = mean(&|r| r.structure);
    let never_settled = runs.iter().all(|r| !r.settled);

    let zoo = census.species();
    let count = |kind: Kind| zoo.iter().filter(|s| s.kind == kind).count();
    let (ships, oscillators, still_lifes) = (count(Kind::Spaceship), count(Kind::Oscillator), count(Kind::Still));

    let temperament = if extinct >= 0.5 {
        Temperament::Extinct
    } else if lifespan < 0.25 {
        Temperament::Frozen
    } else if !never_settled {
        Temperament::SlowBurn
    } else if motion < 0.002 {
        Temperament::Simmering
    } else if motion > 0.15 {
        Temperament::Chaotic
    } else if structure >= 0.4 {
        Temperament::Coagulating
    } else if density >= 0.15 {
        Temperament::Teeming
    } else {
        Temperament::Complex
    };
    let score = score(dynamics(lifespan, motion, density, extinct), zoo_richness(&zoo));

    Specimen {
        rule: rule.to_string(),
        name: rule.name(),
        score,
        temperament,
        lifespan,
        activity,
        motion,
        density,
        structure,
        extinct,
        ships,
        oscillators,
        still_lifes,
        zoo,
        screen_score: screen(rule, cfg),
        screen_rank: None,
        value: rule,
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Stage {
    Screen,
    Study,
}

pub struct Hunt {
    pub config: Config,
    pub screened: usize,
    /// Every closely studied rule, best first.
    pub specimens: Vec<Specimen>,
    pub seconds: f64,
}

/// The rules to screen: all of them, or a reproducible random sample.
fn rules_to_screen(cfg: &Config) -> Vec<Rule> {
    let mut all: Vec<u32> = (0..RULES_WITHOUT_B0).collect();
    if let Some(n) = cfg.sample.filter(|&n| n < all.len()) {
        let mut rng = Rng::for_parts(&[cfg.seed, 0x5a3b1e]);
        for i in 0..n {
            let j = i + rng.below(all.len() - i);
            all.swap(i, j);
        }
        all.truncate(n);
        all.sort_unstable();
    }
    all.into_iter().map(Rule::from_index).collect()
}

/// Runs the whole hunt. `progress` is called from worker threads with (stage, done, total).
pub fn hunt(cfg: &Config, progress: impl Fn(Stage, usize, usize) + Sync) -> Hunt {
    let start = Instant::now();
    let rules = rules_to_screen(cfg);
    let done = AtomicUsize::new(0);
    let total = rules.len();
    let mut screened: Vec<(Rule, f64)> = rules
        .par_iter()
        .map(|&rule| {
            let score = screen(rule, cfg);
            let d = done.fetch_add(1, Ordering::Relaxed) + 1;
            if d.is_multiple_of(1024) || d == total {
                progress(Stage::Screen, d, total);
            }
            (rule, score)
        })
        .collect();
    screened.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.index().cmp(&b.0.index())));

    // Named rules always get a close look too, so the gallery can show where they stand.
    let mut finalists: Vec<Rule> = screened.iter().take(cfg.finalists).map(|(r, _)| *r).collect();
    for (rule, _) in named_rules() {
        if !finalists.contains(&rule) {
            finalists.push(rule);
        }
    }

    let done = AtomicUsize::new(0);
    let total = finalists.len();
    let mut specimens: Vec<Specimen> = finalists
        .par_iter()
        .map(|&rule| {
            let mut specimen = evaluate(rule, cfg);
            specimen.screen_rank = screened.iter().position(|(r, _)| *r == rule).map(|p| p + 1);
            let d = done.fetch_add(1, Ordering::Relaxed) + 1;
            if d.is_multiple_of(16) || d == total {
                progress(Stage::Study, d, total);
            }
            specimen
        })
        .collect();
    specimens.sort_by(|a, b| b.score.total_cmp(&a.score).then(a.value.index().cmp(&b.value.index())));

    Hunt { config: cfg.clone(), screened: screened.len(), specimens, seconds: start.elapsed().as_secs_f64() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny() -> Config {
        Config {
            sample: Some(64),
            finalists: 4,
            screen_size: 32,
            screen_steps: 64,
            soup_size: 48,
            soup_steps: 160,
            soups: 1,
            nurseries: 6,
            nursery_size: 40,
            nursery_steps: 120,
            ..Config::default()
        }
    }

    #[test]
    fn extinct_and_frozen_rules_score_low() {
        let cfg = tiny();
        let dies: Rule = "B/S".parse().unwrap(); // nothing is ever born, everything dies
        let s = evaluate(dies, &cfg);
        assert_eq!(s.temperament, Temperament::Extinct);
        assert_eq!(s.score, 0.0);
        let frozen: Rule = "B5678/S45678".parse().unwrap(); // Majority: soups anneal into blobs
        assert!(evaluate(frozen, &cfg).lifespan < 0.5);
    }

    #[test]
    fn life_finds_gliders_and_beats_noise() {
        let cfg = Config { soup_size: 96, soup_steps: 300, nurseries: 24, ..tiny() };
        let life = evaluate(Rule::LIFE, &cfg);
        assert!(life.zoo.iter().any(|s| s.speed.as_deref() == Some("c/4 diagonal")), "no glider found");
        assert!(life.still_lifes >= 3 && life.oscillators >= 1, "{life:?}");
        assert_eq!(life.temperament, Temperament::Complex);
        let seeds = evaluate("B2/S".parse().unwrap(), &cfg);
        assert_eq!(seeds.temperament, Temperament::Chaotic);
        assert!(life.score > seeds.score + 20.0, "Life {} vs Seeds {}", life.score, seeds.score);
    }

    #[test]
    fn liveliness_rewards_the_middle_band() {
        assert_eq!(liveliness(0.0), 0.0);
        assert_eq!(liveliness(0.0005), 0.0);
        assert_eq!(liveliness(0.01), 1.0);
        assert_eq!(liveliness(0.08), 1.0);
        assert!((liveliness(0.5) - 0.1).abs() < 1e-9);
        assert!(liveliness(0.2) > 0.1 && liveliness(0.2) < 1.0);
    }

    #[test]
    fn hunt_is_reproducible() {
        let cfg = tiny();
        let a = hunt(&cfg, |_, _, _| {});
        let b = hunt(&cfg, |_, _, _| {});
        assert_eq!(a.screened, 64);
        let rules = |h: &Hunt| h.specimens.iter().map(|s| (s.rule.clone(), s.score)).collect::<Vec<_>>();
        assert_eq!(rules(&a), rules(&b));
        // The 4 finalists plus every named rule, some of which may be among the finalists.
        let named = named_rules().len();
        assert!((named..=named + 4).contains(&a.specimens.len()), "{} studied", a.specimens.len());
        assert!(a.specimens.windows(2).all(|w| w[0].score >= w[1].score));
    }
}
