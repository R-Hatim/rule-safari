//! Plays a rule in the terminal, two cells per character.

use std::io::{self, IsTerminal, Write};
use std::thread;
use std::time::Duration;

use crate::census::Shape;
use crate::rule::Rule;
use crate::sim::{Rng, Sim};

pub struct Options {
    pub width: usize,
    pub height: usize,
    pub generations: u32,
    pub fps: u32,
    pub density: f64,
    pub seed: u64,
}

fn half_block(top: bool, bottom: bool) -> char {
    match (top, bottom) {
        (true, true) => '█',
        (true, false) => '▀',
        (false, true) => '▄',
        (false, false) => ' ',
    }
}

/// The grid as text: each character shows a cell above a cell.
pub fn render(sim: &Sim) -> String {
    let mut out = String::with_capacity((sim.width + 1) * sim.height.div_ceil(2) * 3);
    for y in (0..sim.height).step_by(2) {
        for x in 0..sim.width {
            out.push(half_block(sim.get(x, y), y + 1 < sim.height && sim.get(x, y + 1)));
        }
        out.push('\n');
    }
    out
}

/// A creature drawn the same way, one string per line.
pub fn draw_shape(cells: &Shape) -> Vec<String> {
    let w = cells.iter().map(|c| c.0 as usize + 1).max().unwrap_or(0);
    let h = cells.iter().map(|c| c.1 as usize + 1).max().unwrap_or(0);
    let alive = |x: usize, y: usize| cells.binary_search(&(x as u8, y as u8)).is_ok();
    (0..h).step_by(2).map(|y| (0..w).map(|x| half_block(alive(x, y), alive(x, y + 1))).collect()).collect()
}

/// Animates a random soup. When stdout isn't a terminal, prints only the last frame, plainly.
pub fn run(rule: Rule, opts: &Options) -> io::Result<()> {
    let mut sim = Sim::new(rule, opts.width, opts.height);
    let mut rng = Rng::for_parts(&[opts.seed, rule.index() as u64, 0x3a7c]);
    sim.fill_random(0, 0, opts.width, opts.height, opts.density, &mut rng);
    let title = match rule.name() {
        Some(name) => format!("{rule} · {name}"),
        None => rule.to_string(),
    };
    let interactive = io::stdout().is_terminal();
    let mut out = io::stdout().lock();
    if !interactive {
        for _ in 0..opts.generations {
            sim.step();
        }
        write!(out, "{title} · generation {} · population {}\n{}", opts.generations, sim.population(), render(&sim))?;
        return Ok(());
    }
    let delay = Duration::from_secs_f64(1.0 / opts.fps.max(1) as f64);
    write!(out, "\x1b[2J")?;
    for generation in 0..=opts.generations {
        writeln!(
            out,
            "\x1b[H\x1b[38;5;150m{}\x1b[0m{title} · generation {generation} · population {}\x1b[K",
            render(&sim),
            sim.population()
        )?;
        out.flush()?;
        if generation < opts.generations {
            sim.step();
            thread::sleep(delay);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_two_rows_per_line() {
        let mut sim = Sim::new(Rule::LIFE, 4, 3);
        sim.set(0, 0, true);
        sim.set(1, 1, true);
        sim.set(2, 0, true);
        sim.set(2, 1, true);
        sim.set(3, 2, true);
        assert_eq!(render(&sim), "▀▄█ \n   ▀\n");
    }

    #[test]
    fn draws_a_glider() {
        let glider: Shape = vec![(0, 2), (1, 0), (1, 2), (2, 1), (2, 2)];
        assert_eq!(draw_shape(&glider), vec![" ▀▄", "▀▀▀"]);
    }
}
