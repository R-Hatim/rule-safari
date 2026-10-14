//! Life-like rules: two states, Moore neighbourhood, outer-totalistic, in B/S notation.

use std::fmt;
use std::str::FromStr;

/// Rules without a B0 term: 2^8 birth sets (B1..B8) times 2^9 survival sets (S0..S8).
pub const RULES_WITHOUT_B0: u32 = 1 << 17;

/// A rule as two 9-bit masks. Bit n of `birth`: a dead cell with n live neighbours
/// comes alive. Bit n of `survive`: a live cell with n live neighbours stays alive.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Rule {
    pub birth: u16,
    pub survive: u16,
}

impl Rule {
    pub const LIFE: Rule = Rule { birth: 1 << 3, survive: (1 << 2) | (1 << 3) };

    pub fn new(birth: u16, survive: u16) -> Rule {
        Rule { birth: birth & 0x1ff, survive: survive & 0x1ff }
    }

    /// Rule number `i` of the B0-free space, `0 <= i < RULES_WITHOUT_B0`.
    pub fn from_index(i: u32) -> Rule {
        Rule::new(((i & 0xff) << 1) as u16, (i >> 8) as u16)
    }

    /// Inverse of [`Rule::from_index`]. Meaningless for rules with B0.
    pub fn index(self) -> u32 {
        (self.birth as u32 >> 1) | ((self.survive as u32) << 8)
    }

    pub fn has_b0(self) -> bool {
        self.birth & 1 != 0
    }

    /// Next state, indexed by `alive * 9 + live_neighbours`.
    pub fn table(self) -> [u8; 18] {
        let mut t = [0u8; 18];
        for n in 0..9 {
            t[n] = ((self.birth >> n) & 1) as u8;
            t[9 + n] = ((self.survive >> n) & 1) as u8;
        }
        t
    }

    /// The common name of a well-known rule, if it has one.
    pub fn name(self) -> Option<&'static str> {
        named_rules().into_iter().find(|(r, _)| *r == self).map(|(_, n)| n)
    }
}

/// Well-known named rules, as listed on LifeWiki's "List of Life-like cellular automata".
const NAMED: &[(&str, &str)] = &[
    ("B3/S23", "Life"),
    ("B36/S23", "HighLife"),
    ("B3678/S34678", "Day & Night"),
    ("B2/S", "Seeds"),
    ("B3/S012345678", "Life without Death"),
    ("B34/S34", "34 Life"),
    ("B35678/S5678", "Diamoeba"),
    ("B36/S125", "2x2"),
    ("B368/S245", "Morley"),
    ("B4678/S35678", "Anneal"),
    ("B3/S12345", "Maze"),
    ("B3/S1234", "Mazectric"),
    ("B3/S45678", "Coral"),
    ("B1357/S1357", "Replicator"),
    ("B1357/S02468", "Fredkin"),
    ("B1/S1", "Gnarl"),
    ("B345/S5", "Long Life"),
    ("B357/S1358", "Amoeba"),
    ("B357/S238", "Pseudo Life"),
    ("B234/S", "Serviettes"),
    ("B2/S0", "Live Free or Die"),
    ("B3678/S235678", "Stains"),
    ("B378/S235678", "Coagulations"),
    ("B45678/S2345", "Walled Cities"),
    ("B5678/S45678", "Majority"),
    ("B37/S23", "DryLife"),
    ("B38/S23", "Pedestrian Life"),
    ("B3/S12", "Flock"),
    ("B1/S012345678", "H-trees"),
    ("B25678/S5678", "Iceballs"),
    ("B3457/S4568", "Gems"),
    ("B35678/S4678", "Holstein"),
    ("B345/S4567", "Assimilation"),
    ("B378/S012345678", "Plow World"),
];

/// All named rules, parsed.
pub fn named_rules() -> Vec<(Rule, &'static str)> {
    NAMED
        .iter()
        .map(|(r, n)| (r.parse().expect("named rule strings are valid"), *n))
        .collect()
}

fn digits(mask: u16) -> String {
    (0..9).filter(|n| mask >> n & 1 == 1).map(|n| char::from(b'0' + n as u8)).collect()
}

fn mask(digits: &str) -> Result<u16, String> {
    let mut m = 0u16;
    for ch in digits.chars() {
        match ch.to_digit(10) {
            Some(d) if d <= 8 => m |= 1 << d,
            _ => return Err(format!("'{ch}' is not a neighbour count (0-8)")),
        }
    }
    Ok(m)
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "B{}/S{}", digits(self.birth), digits(self.survive))
    }
}

impl FromStr for Rule {
    type Err = String;

    /// Accepts `B3/S23`, `b3s23`, `S23/B3`, and the classic survival/birth form `23/3`.
    fn from_str(s: &str) -> Result<Rule, String> {
        let s = s.trim().to_ascii_uppercase();
        if s.is_empty() {
            return Err("empty rule".into());
        }
        if !s.contains(['B', 'S']) {
            let (survive, birth) = s
                .split_once('/')
                .ok_or_else(|| format!("'{s}' is not in B/S notation (e.g. B3/S23)"))?;
            return Ok(Rule::new(mask(birth)?, mask(survive)?));
        }
        let (mut birth, mut survive) = (None, None);
        let mut section: Option<(char, String)> = None;
        let mut flush = |section: &mut Option<(char, String)>| -> Result<(), String> {
            if let Some((letter, ds)) = section.take() {
                let slot = if letter == 'B' { &mut birth } else { &mut survive };
                if slot.is_some() {
                    return Err(format!("'{letter}' appears twice"));
                }
                *slot = Some(mask(&ds)?);
            }
            Ok(())
        };
        for ch in s.chars() {
            match ch {
                'B' | 'S' => {
                    flush(&mut section)?;
                    section = Some((ch, String::new()));
                }
                '/' => flush(&mut section)?,
                d if d.is_ascii_digit() => match &mut section {
                    Some((_, ds)) => ds.push(d),
                    None => return Err(format!("digit '{d}' before B or S")),
                },
                other => return Err(format!("unexpected '{other}' in rule")),
            }
        }
        flush(&mut section)?;
        match (birth, survive) {
            (Some(b), Some(s)) => Ok(Rule::new(b, s)),
            _ => Err("a rule needs both a B and an S part (e.g. B3/S23)".into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_spellings() {
        for s in ["B3/S23", "b3/s23", "B3S23", "S23/B3", "23/3", " B3/S23 "] {
            assert_eq!(s.parse::<Rule>().unwrap(), Rule::LIFE, "{s}");
        }
        assert_eq!("B2/S".parse::<Rule>().unwrap(), Rule::new(1 << 2, 0));
    }

    #[test]
    fn rejects_garbage() {
        for s in ["", "B9/S23", "hello", "B3", "B3/S23/B4", "3B/S23", "B3/S2x"] {
            assert!(s.parse::<Rule>().is_err(), "{s} should not parse");
        }
    }

    #[test]
    fn display_round_trips_every_rule() {
        for i in 0..RULES_WITHOUT_B0 {
            let r = Rule::from_index(i);
            assert!(!r.has_b0());
            assert_eq!(r.index(), i);
            assert_eq!(r.to_string().parse::<Rule>().unwrap(), r);
        }
    }

    #[test]
    fn life_table() {
        let t = Rule::LIFE.table();
        assert_eq!(t[3], 1, "birth on 3");
        assert_eq!(t[2], 0, "no birth on 2");
        assert_eq!(t[9 + 2], 1, "survive on 2");
        assert_eq!(t[9 + 4], 0, "die on 4");
    }

    #[test]
    fn named_rules_parse_and_are_unique() {
        let named = named_rules();
        let mut rules: Vec<Rule> = named.iter().map(|(r, _)| *r).collect();
        rules.sort();
        rules.dedup();
        assert_eq!(rules.len(), named.len(), "a rule is listed twice");
        assert_eq!(Rule::LIFE.name(), Some("Life"));
        assert!(named.iter().all(|(r, _)| !r.has_b0()));
    }
}
