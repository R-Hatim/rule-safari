//! Writes a hunt's results: `results.json`, and `gallery.html`, a self-contained field guide.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{Value, json};

use crate::census::{Kind, Species};
use crate::hunt::{Hunt, Specimen};

const TEMPLATE: &str = include_str!("../assets/gallery.html");
const PLACEHOLDER: &str = "__SAFARI_DATA__";

/// A notable find, such as the fastest spaceship of the hunt.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub title: &'static str,
    pub rule: String,
    pub name: Option<&'static str>,
    pub detail: String,
    pub creature: Option<Species>,
}

fn record(title: &'static str, s: &Specimen, detail: String, creature: Option<&Species>) -> Record {
    Record { title, rule: s.rule.clone(), name: s.name, detail, creature: creature.cloned() }
}

fn creatures(hunt: &Hunt, kind: Kind) -> impl Iterator<Item = (&Specimen, &Species)> {
    hunt.specimens.iter().flat_map(move |s| s.zoo.iter().filter(move |c| c.kind == kind).map(move |c| (s, c)))
}

pub fn records(hunt: &Hunt) -> Vec<Record> {
    let mut out = Vec::new();
    let ship_detail =
        |c: &Species| format!("{}, {} cells, period {}", c.speed.as_deref().unwrap_or("?"), c.population, c.period);
    let fastest = creatures(hunt, Kind::Spaceship)
        .max_by(|a, b| a.1.speed_value().total_cmp(&b.1.speed_value()).then(b.1.population.cmp(&a.1.population)));
    if let Some((s, c)) = fastest {
        out.push(record("Fastest spaceship", s, ship_detail(c), Some(c)));
    }
    let slowest = creatures(hunt, Kind::Spaceship)
        .min_by(|a, b| a.1.speed_value().total_cmp(&b.1.speed_value()).then(a.1.population.cmp(&b.1.population)));
    if let Some((s, c)) = slowest {
        out.push(record("Slowest spaceship", s, ship_detail(c), Some(c)));
    }
    if let Some((s, c)) =
        creatures(hunt, Kind::Spaceship).find(|(_, c)| c.dx != 0 && c.dy != 0 && c.dx.abs() != c.dy.abs())
    {
        out.push(record("Oblique spaceship", s, ship_detail(c), Some(c)));
    }
    let biggest = creatures(hunt, Kind::Spaceship).max_by_key(|(_, c)| (c.population, std::cmp::Reverse(c.period)));
    if let Some((s, c)) = biggest {
        out.push(record("Biggest spaceship", s, ship_detail(c), Some(c)));
    }
    let longest = creatures(hunt, Kind::Oscillator).max_by_key(|(_, c)| (c.period, std::cmp::Reverse(c.population)));
    if let Some((s, c)) = longest {
        out.push(record(
            "Longest-period oscillator",
            s,
            format!("period {}, {} cells", c.period, c.population),
            Some(c),
        ));
    }
    if let Some(s) = hunt.specimens.iter().max_by_key(|s| (s.ships, s.oscillators, std::cmp::Reverse(s.value.index())))
    {
        let detail =
            format!("{} kinds of spaceship, {} oscillators, {} still lifes", s.ships, s.oscillators, s.still_lifes);
        out.push(record("Richest zoo", s, detail, s.zoo.first()));
    }
    out
}

/// How a rule looks in motion, each feature scaled to roughly 0..1.
fn features(s: &Specimen) -> [f64; 5] {
    [
        (s.motion.max(1e-4).log10() + 4.0) / 4.0,
        s.density,
        s.structure,
        ((1.0 + s.ships as f64).ln() / 7f64.ln()).min(1.0),
        s.lifespan,
    ]
}

/// 1 for rules that behave alike, falling towards 0 as their features drift apart.
fn similarity(a: &Specimen, b: &Specimen) -> f64 {
    let d2: f64 = features(a).iter().zip(features(b)).map(|(x, y)| (x - y).powi(2)).sum();
    (-d2 / 0.12f64.powi(2)).exp()
}

/// Picks up to `n` rules that score well and differ from each other: each pick maximises its
/// score minus a penalty for resembling a rule already picked (maximal marginal relevance).
/// Returns indices into `specimens`, each with the close relatives it stands in for.
pub fn diverse_picks(specimens: &[Specimen], n: usize) -> Vec<(usize, Vec<usize>)> {
    let pool = (n * 4).min(specimens.len());
    let mut picked: Vec<usize> = Vec::new();
    let value = |i: usize, picked: &[usize]| {
        let resemblance = picked.iter().map(|&p| similarity(&specimens[i], &specimens[p])).fold(0.0, f64::max);
        specimens[i].score / 100.0 - 0.35 * resemblance
    };
    while picked.len() < n.min(pool) {
        let best = (0..pool)
            .filter(|i| !picked.contains(i))
            .max_by(|&a, &b| value(a, &picked).total_cmp(&value(b, &picked)).then(b.cmp(&a)));
        match best {
            Some(i) => picked.push(i),
            None => break,
        }
    }
    let mut relatives: Vec<Vec<usize>> = vec![Vec::new(); picked.len()];
    for i in (0..pool).filter(|i| !picked.contains(i)) {
        let closest = picked
            .iter()
            .enumerate()
            .map(|(k, &p)| (k, similarity(&specimens[i], &specimens[p])))
            .max_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((k, _)) = closest.filter(|&(_, sim)| sim > 0.7) {
            relatives[k].push(i);
        }
    }
    picked.into_iter().zip(relatives).collect()
}

/// Picks in ranking order: the diversity pass decides who is shown, not the order.
fn sorted(mut picks: Vec<(usize, Vec<usize>)>) -> Vec<(usize, Vec<usize>)> {
    picks.sort_by_key(|p| p.0);
    picks
}

fn with_rank(s: &Specimen, rank: usize, zoo_limit: usize) -> Value {
    let mut s = s.clone();
    s.zoo.truncate(zoo_limit);
    let mut v = serde_json::to_value(&s).expect("specimens serialize");
    v["rank"] = json!(rank);
    v
}

/// Everything the gallery shows, as JSON.
pub fn gallery_data(hunt: &Hunt, top: usize) -> Value {
    let generated_at = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let named: Vec<Value> = hunt
        .specimens
        .iter()
        .enumerate()
        .filter(|(_, s)| s.name.is_some())
        .map(|(i, s)| {
            json!({
                "rank": i + 1, "rule": s.rule, "name": s.name, "score": s.score,
                "temperament": s.temperament, "ships": s.ships, "oscillators": s.oscillators,
                "stillLifes": s.still_lifes, "screenRank": s.screen_rank,
            })
        })
        .collect();
    let sum = |f: fn(&Specimen) -> usize| hunt.specimens.iter().map(f).sum::<usize>();
    json!({
        "meta": {
            "version": env!("CARGO_PKG_VERSION"),
            "generatedAt": generated_at,
            "seconds": hunt.seconds,
            "screened": hunt.screened,
            "studied": hunt.specimens.len(),
            "config": hunt.config,
        },
        "totals": {
            "ships": sum(|s| s.ships),
            "oscillators": sum(|s| s.oscillators),
            "stillLifes": sum(|s| s.still_lifes),
            "rulesWithShips": hunt.specimens.iter().filter(|s| s.ships > 0).count(),
        },
        "records": records(hunt),
        "specimens": sorted(diverse_picks(&hunt.specimens, top))
            .into_iter()
            .map(|(i, relatives)| {
                let mut v = with_rank(&hunt.specimens[i], i + 1, 8);
                v["relatives"] = json!(relatives.iter().take(6).map(|&r| &hunt.specimens[r].rule).collect::<Vec<_>>());
                v
            })
            .collect::<Vec<_>>(),
        "named": named,
    })
}

/// Puts the data into the page. `</` is escaped so the JSON can't close its script tag.
pub fn render_gallery(data: &Value) -> String {
    let json = serde_json::to_string(data).expect("gallery data serializes").replace("</", "<\\/");
    assert!(TEMPLATE.contains(PLACEHOLDER), "gallery template lost its data placeholder");
    TEMPLATE.replacen(PLACEHOLDER, &json, 1)
}

/// Writes `results.json` (every studied rule) and `gallery.html` (the best `top`) into `dir`.
pub fn write(hunt: &Hunt, dir: &Path, top: usize) -> io::Result<(PathBuf, PathBuf)> {
    fs::create_dir_all(dir)?;
    let mut results = gallery_data(hunt, usize::MAX);
    results["specimens"] =
        json!(hunt.specimens.iter().enumerate().map(|(i, s)| with_rank(s, i + 1, 16)).collect::<Vec<_>>());
    let results_path = dir.join("results.json");
    fs::write(&results_path, serde_json::to_string_pretty(&results).expect("results serialize"))?;
    let gallery_path = dir.join("gallery.html");
    fs::write(&gallery_path, render_gallery(&gallery_data(hunt, top)))?;
    Ok((results_path, gallery_path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hunt::{Config, hunt};

    fn small_hunt() -> Hunt {
        let cfg = Config {
            sample: Some(32),
            finalists: 3,
            screen_size: 24,
            screen_steps: 48,
            soup_size: 64,
            soup_steps: 200,
            soups: 1,
            nurseries: 8,
            nursery_size: 40,
            nursery_steps: 100,
            ..Config::default()
        };
        hunt(&cfg, |_, _, _| {})
    }

    #[test]
    fn gallery_embeds_data_safely() {
        let page = render_gallery(&json!({ "evil": "</script><script>alert(1)</script>" }));
        assert!(!page.contains(PLACEHOLDER));
        assert!(!page.contains("</script><script>alert"));
        assert!(page.contains("<\\/script>"));
    }

    #[test]
    fn data_has_ranks_records_and_named_rules() {
        let hunt = small_hunt();
        let data = gallery_data(&hunt, 5);
        let specimens = data["specimens"].as_array().unwrap();
        assert_eq!(specimens.len(), 5);
        assert_eq!(specimens[0]["rank"], 1);
        assert!(specimens.iter().all(|s| s["zoo"].as_array().unwrap().len() <= 8));
        assert!(data["named"].as_array().unwrap().iter().any(|n| n["name"] == "Life"));
        let titles: Vec<&str> =
            data["records"].as_array().unwrap().iter().map(|r| r["title"].as_str().unwrap()).collect();
        assert!(titles.contains(&"Richest zoo"), "{titles:?}");
    }

    #[test]
    fn picks_are_distinct_and_start_with_the_best() {
        let hunt = small_hunt();
        let picks = diverse_picks(&hunt.specimens, 10);
        assert_eq!(picks.len(), 10);
        assert_eq!(picks[0].0, 0, "the top-scoring rule is always the first pick");
        let mut seen: Vec<usize> = picks.iter().map(|p| p.0).collect();
        for (_, relatives) in &picks {
            seen.extend(relatives);
        }
        let count = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), count, "a rule is both picked and a relative, or a relative twice");
    }

    #[test]
    fn writes_both_files() {
        let dir = std::env::temp_dir().join(format!("rule-safari-test-{}", std::process::id()));
        let (json_path, html_path) = write(&small_hunt(), &dir, 4).unwrap();
        let parsed: Value = serde_json::from_str(&fs::read_to_string(&json_path).unwrap()).unwrap();
        assert!(parsed["specimens"].as_array().unwrap().len() > 4, "results.json keeps every studied rule");
        assert!(fs::read_to_string(&html_path).unwrap().contains("\"specimens\""));
        fs::remove_dir_all(&dir).unwrap();
    }
}
