//! rule-safari: hunts through the Life-like cellular automata for the interesting ones.

mod census;
mod hunt;
mod metrics;
mod report;
mod rule;
mod sim;
mod watch;

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use crate::census::Kind;
use crate::hunt::{Config, Specimen, Stage, Temperament};
use crate::rule::Rule;

#[derive(Parser)]
#[command(
    name = "rule-safari",
    version,
    about = "Hunts through the 131,072 Life-like cellular automata for the interesting ones."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Screen every rule, study the best closely, and write a field-guide gallery.
    Hunt {
        /// Folder for gallery.html and results.json.
        #[arg(long, default_value = "safari")]
        out: PathBuf,
        /// Same seed, same hunt.
        #[arg(long, default_value_t = 1)]
        seed: u64,
        /// How many rules the gallery shows.
        #[arg(long, default_value_t = 48)]
        top: usize,
        /// How many of the best-screened rules get a close look.
        #[arg(long)]
        finalists: Option<usize>,
        /// Screen a random sample of this many rules instead of all 131,072.
        #[arg(long)]
        sample: Option<usize>,
        /// A smaller hunt that takes seconds: 4,096 sampled rules and smaller soups.
        #[arg(long)]
        quick: bool,
    },
    /// Study one rule: how its soups behave and which creatures live in it.
    Inspect {
        /// The rule, e.g. B3/S23, B36/S23 or 23/3.
        rule: Rule,
        #[arg(long, default_value_t = 1)]
        seed: u64,
    },
    /// Watch a rule evolve in the terminal.
    Watch {
        rule: Rule,
        #[arg(long, default_value_t = 96)]
        width: usize,
        #[arg(long, default_value_t = 48)]
        height: usize,
        #[arg(long, default_value_t = 500)]
        generations: u32,
        #[arg(long, default_value_t = 20)]
        fps: u32,
        /// Fraction of cells alive at the start.
        #[arg(long, default_value_t = 0.375)]
        density: f64,
        #[arg(long, default_value_t = 1)]
        seed: u64,
    },
}

fn temperament_note(t: Temperament, steps: u32) -> String {
    match t {
        Temperament::Extinct => "extinct: soups die out".into(),
        Temperament::Frozen => "frozen: soups lock up almost at once".into(),
        Temperament::SlowBurn => "slow burn: busy for a while, then settles".into(),
        Temperament::Simmering => {
            format!("simmering: never settles in {steps} generations, but only flickers in place")
        }
        Temperament::Coagulating => format!("coagulating: lively for all {steps} generations, clumping into blobs"),
        Temperament::Teeming => format!("teeming: dense, and stirring everywhere for all {steps} generations"),
        Temperament::Complex => {
            format!("complex: lively for all {steps} generations, things travel through sparse space")
        }
        Temperament::Chaotic => format!("chaotic: never settles in {steps} generations, and looks like noise"),
    }
}

fn title(s: &Specimen) -> String {
    match s.name {
        Some(name) => format!("{} · {name}", s.rule),
        None => s.rule.clone(),
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn pct(f: f64) -> String {
    format!("{:.1}%", f * 100.0)
}

fn inspect(rule: Rule, seed: u64) {
    let cfg = Config { seed, ..Config::default() };
    let s = hunt::evaluate(rule, &cfg);
    println!("{}", title(&s));
    if rule.has_b0() {
        println!("  note         B0 rules make empty space flash, so the census below is unreliable");
    }
    println!("  temperament  {}", temperament_note(s.temperament, cfg.soup_steps));
    println!("  score        {:.1}   (quick screen: {:.1})", s.score, s.screen_score);
    println!(
        "  soups        lifespan {}  activity {}  motion {}  density {}  structure {:.2}  extinct {}",
        pct(s.lifespan),
        pct(s.activity),
        pct(s.motion),
        pct(s.density),
        s.structure,
        pct(s.extinct)
    );
    println!(
        "  zoo          {} · {} · {}",
        plural(s.ships, "spaceship", "spaceships"),
        plural(s.oscillators, "oscillator", "oscillators"),
        plural(s.still_lifes, "still life", "still lifes")
    );
    if s.zoo.is_empty() {
        return;
    }
    println!();
    for c in &s.zoo {
        let what = match c.kind {
            Kind::Spaceship => format!("spaceship   {:<18} period {:<3}", c.speed.as_deref().unwrap_or(""), c.period),
            Kind::Oscillator => format!("oscillator  {:<18} period {:<3}", "", c.period),
            Kind::Still => format!("still life  {:<18} {:<10}", "", ""),
        };
        println!("  {what} {:>3} cells   seen {}×", c.population, c.sightings);
    }
    let showcase: Vec<_> = s.zoo.iter().filter(|c| c.kind != Kind::Still).take(6).collect();
    if !showcase.is_empty() {
        println!();
        let drawings: Vec<Vec<String>> = showcase.iter().map(|c| watch::draw_shape(&c.cells)).collect();
        let widths: Vec<usize> = showcase.iter().map(|c| c.width.max(4)).collect();
        let lines = drawings.iter().map(Vec::len).max().unwrap_or(0);
        for line in 0..lines {
            let row: Vec<String> = drawings
                .iter()
                .zip(&widths)
                .map(|(d, &w)| format!("{:<w$}", d.get(line).map(String::as_str).unwrap_or("")))
                .collect();
            println!("  {}", row.join("   ").trim_end());
        }
    }
}

fn run_hunt(cfg: Config, out: PathBuf, top: usize) -> std::io::Result<()> {
    let space = match cfg.sample {
        Some(n) => format!("a sample of {n} rules"),
        None => format!("all {} rules without B0", rule::RULES_WITHOUT_B0),
    };
    eprintln!("Screening {space} on {}x{} soups...", cfg.screen_size, cfg.screen_size);
    let result = hunt::hunt(&cfg, |stage, done, total| {
        let label = match stage {
            Stage::Screen => "screening",
            Stage::Study => "studying finalists",
        };
        eprint!("\r  {label}: {done}/{total}   ");
        if done == total {
            eprintln!();
        }
        let _ = std::io::stderr().flush();
    });
    let (json_path, html_path) = report::write(&result, &out, top)?;
    println!(
        "Screened {} rules and studied {} closely in {:.1}s.\n",
        result.screened,
        result.specimens.len(),
        result.seconds
    );
    println!("  rank  score  rule                        temperament  ships  oscillators");
    for (i, s) in result.specimens.iter().take(12).enumerate() {
        let temperament =
            serde_json::to_value(s.temperament).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default();
        println!(
            "  {:>4}  {:>5.1}  {:<26}  {:<11}  {:>5}  {:>11}",
            i + 1,
            s.score,
            title(s),
            temperament,
            s.ships,
            s.oscillators
        );
    }
    if let Some(i) = result.specimens.iter().position(|s| s.value == Rule::LIFE) {
        println!("\n  Conway's Life ranks #{} of {} studied.", i + 1, result.specimens.len());
    }
    println!("\nGallery: {}\nData:    {}", html_path.display(), json_path.display());
    Ok(())
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let outcome = match cli.command {
        Command::Hunt { out, seed, top, finalists, sample, quick } => {
            let base = if quick { Config::quick() } else { Config::default() };
            let cfg =
                Config { seed, finalists: finalists.unwrap_or(base.finalists), sample: sample.or(base.sample), ..base };
            run_hunt(cfg, out, top)
        }
        Command::Inspect { rule, seed } => {
            inspect(rule, seed);
            Ok(())
        }
        Command::Watch { rule, width, height, generations, fps, density, seed } => {
            if width < 3 || height < 3 {
                eprintln!("error: the grid must be at least 3x3");
                return ExitCode::FAILURE;
            }
            watch::run(rule, &watch::Options { width, height, generations, fps, density, seed })
        }
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
