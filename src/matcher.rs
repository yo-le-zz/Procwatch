//! Comparaison de noms de processus (insensible à la casse, `.exe` optionnel,
//! jokers `*` en début et/ou fin de motif).

use crate::event::image_basename;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Pattern {
    Exact(String),
    Prefix(String),
    Suffix(String),
    Contains(String),
}

/// Ensemble de motifs de noms de processus.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NameMatcher {
    patterns: Vec<Pattern>,
}

/// Met un nom en forme canonique : minuscules, sans chemin, sans `.exe` final.
fn normalize(name: &str) -> String {
    let lower = image_basename(name.trim()).to_lowercase();
    lower
        .strip_suffix(".exe")
        .map(str::to_owned)
        .unwrap_or(lower)
}

impl NameMatcher {
    /// Construit un matcher. Les motifs vides (ou réduits à `*`) sont ignorés :
    /// un `*` seul ciblerait ou ignorerait tout, ce qui n'est jamais voulu.
    pub fn new<S: AsRef<str>>(patterns: &[S]) -> Self {
        let patterns = patterns
            .iter()
            .filter_map(|raw| Self::parse(raw.as_ref()))
            .collect();
        Self { patterns }
    }

    fn parse(raw: &str) -> Option<Pattern> {
        let starts = raw.trim().starts_with('*');
        let ends = raw.trim().ends_with('*');
        let core = normalize(raw.trim().trim_matches('*'));
        if core.is_empty() {
            return None;
        }
        Some(match (starts, ends) {
            (true, true) => Pattern::Contains(core),
            (true, false) => Pattern::Suffix(core),
            (false, true) => Pattern::Prefix(core),
            (false, false) => Pattern::Exact(core),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    pub fn is_match(&self, image: &str) -> bool {
        let name = normalize(image);
        self.patterns.iter().any(|p| match p {
            Pattern::Exact(x) => name == *x,
            Pattern::Prefix(x) => name.starts_with(x.as_str()),
            Pattern::Suffix(x) => name.ends_with(x.as_str()),
            Pattern::Contains(x) => name.contains(x.as_str()),
        })
    }
}

/// Découpe une saisie « a, b;c » en liste de noms propres.
pub fn split_list(input: &str) -> Vec<String> {
    input
        .split([',', ';'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match_ignores_case_and_extension() {
        let m = NameMatcher::new(&["RobloxPlayerBeta"]);
        assert!(m.is_match("robloxplayerbeta.exe"));
        assert!(m.is_match("ROBLOXPLAYERBETA.EXE"));
        assert!(!m.is_match("robloxplayerbeta2.exe"));
    }

    #[test]
    fn pattern_with_extension_matches_name_without() {
        let m = NameMatcher::new(&["steam.exe"]);
        assert!(m.is_match("Steam"));
        assert!(m.is_match(r"C:\Program Files\Steam\steam.exe"));
    }

    #[test]
    fn wildcards() {
        let m = NameMatcher::new(&["steam*", "*helper", "*node*"]);
        assert!(m.is_match("steamwebhelper.exe"));
        assert!(m.is_match("crashhelper.exe"));
        assert!(m.is_match("a-node-b.exe"));
        assert!(!m.is_match("spotify.exe"));
    }

    #[test]
    fn empty_and_star_only_patterns_are_dropped() {
        let m = NameMatcher::new(&["", "  ", "*", "**"]);
        assert!(m.is_empty());
        assert!(!m.is_match("anything.exe"));
    }

    #[test]
    fn split_list_trims_and_drops_empty() {
        assert_eq!(
            split_list(" a.exe, b ;; c.exe ,"),
            vec!["a.exe", "b", "c.exe"]
        );
        assert!(split_list(" , ; ").is_empty());
    }
}
