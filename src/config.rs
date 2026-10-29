//! Configuration persistante (fichier TOML) et valeurs par défaut.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

/// Processus bruyants masqués par défaut : ils se relancent en rafale sans
/// rapport avec ce que l'on cherche à diagnostiquer.
pub const DEFAULT_IGNORE: &[&str] = &[
    "NetCfgNotifyObjectHost.exe",
    "conhost.exe",
    "RuntimeBroker.exe",
    "backgroundTaskHost.exe",
    "taskhostw.exe",
    "WmiPrvSE.exe",
];

pub const MIN_CONTEXT_SECONDS: u64 = 1;
pub const MAX_CONTEXT_SECONDS: u64 = 600;
pub const MIN_BUFFER_CAPACITY: usize = 100;
pub const MAX_BUFFER_CAPACITY: usize = 1_000_000;

/// Réglages de `procwatch`. Toute clé absente du fichier prend sa valeur par défaut ;
/// une clé inconnue est refusée pour signaler les fautes de frappe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Noms de processus à surveiller (`.exe` optionnel, jokers `*` acceptés).
    pub targets: Vec<String>,
    /// Noms de processus à masquer.
    pub ignore: Vec<String>,
    /// Secondes d'historique affichées quand une cible se termine.
    pub context_seconds: u64,
    /// Nombre maximal d'événements gardés en mémoire pour ce contexte.
    pub buffer_capacity: usize,
    /// Afficher aussi les processus de la liste d'exclusion.
    pub show_ignored: bool,
    /// Afficher les changements d'interfaces réseau.
    pub show_network: bool,
    /// Consulter le journal d'événements Windows quand une cible plante.
    pub query_event_log: bool,
    /// Fichier où écrire une copie du journal (texte brut, sans couleurs).
    pub log_file: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            targets: Vec::new(),
            ignore: DEFAULT_IGNORE.iter().map(|s| (*s).to_owned()).collect(),
            context_seconds: 30,
            buffer_capacity: 5_000,
            show_ignored: false,
            show_network: true,
            query_event_log: true,
            log_file: None,
        }
    }
}

impl Config {
    /// Emplacement par défaut : `%APPDATA%\procwatch\config.toml` sous Windows,
    /// `$XDG_CONFIG_HOME` ou `~/.config` ailleurs (utile pour développer).
    pub fn default_path() -> Option<PathBuf> {
        let base = if cfg!(windows) {
            std::env::var_os("APPDATA").map(PathBuf::from)
        } else {
            std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        };
        base.map(|b| b.join("procwatch").join("config.toml"))
    }

    pub fn from_toml_str(text: &str) -> Result<Self> {
        let config: Self = toml::from_str(text).context("configuration TOML invalide")?;
        config.validate()?;
        Ok(config)
    }

    pub fn to_toml_string(&self) -> Result<String> {
        toml::to_string_pretty(self).context("sérialisation de la configuration impossible")
    }

    /// Charge un fichier ; un fichier absent donne la configuration par défaut.
    pub fn load(path: &Path) -> Result<Self> {
        match fs::read_to_string(path) {
            Ok(text) => {
                Self::from_toml_str(&text).with_context(|| format!("lecture de {}", path.display()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("lecture de {}", path.display())),
        }
    }

    /// Enregistre de façon atomique (fichier temporaire puis renommage) pour ne
    /// jamais laisser une configuration à moitié écrite.
    pub fn save(&self, path: &Path) -> Result<()> {
        let text = self.to_toml_string()?;
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)
                .with_context(|| format!("création du dossier {}", dir.display()))?;
        }
        let tmp = path.with_extension("toml.tmp");
        fs::write(&tmp, text).with_context(|| format!("écriture de {}", tmp.display()))?;
        fs::rename(&tmp, path).with_context(|| format!("remplacement de {}", path.display()))?;
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        if !(MIN_CONTEXT_SECONDS..=MAX_CONTEXT_SECONDS).contains(&self.context_seconds) {
            bail!(
                "context_seconds doit être compris entre {MIN_CONTEXT_SECONDS} et {MAX_CONTEXT_SECONDS} (valeur : {})",
                self.context_seconds
            );
        }
        if !(MIN_BUFFER_CAPACITY..=MAX_BUFFER_CAPACITY).contains(&self.buffer_capacity) {
            bail!(
                "buffer_capacity doit être compris entre {MIN_BUFFER_CAPACITY} et {MAX_BUFFER_CAPACITY} (valeur : {})",
                self.buffer_capacity
            );
        }
        Ok(())
    }

    /// Vérifie qu'il y a de quoi lancer une surveillance.
    pub fn validate_for_run(&self) -> Result<()> {
        self.validate()?;
        if self.targets.iter().all(|t| t.trim().is_empty()) {
            bail!("aucune cible : indiquez au moins un nom de processus à surveiller");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("procwatch-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir.join("config.toml")
    }

    #[test]
    fn default_roundtrips_through_toml() {
        let c = Config::default();
        let parsed = Config::from_toml_str(&c.to_toml_string().unwrap()).unwrap();
        assert_eq!(c, parsed);
    }

    #[test]
    fn partial_file_uses_defaults() {
        let c = Config::from_toml_str("targets = [\"roblox\"]").unwrap();
        assert_eq!(c.targets, vec!["roblox"]);
        assert_eq!(c.context_seconds, 30);
        assert!(!c.ignore.is_empty());
    }

    #[test]
    fn unknown_key_is_rejected() {
        let err = Config::from_toml_str("target = [\"typo\"]").unwrap_err();
        assert!(format!("{err:#}").contains("target"));
    }

    #[test]
    fn out_of_range_values_are_rejected() {
        assert!(Config::from_toml_str("context_seconds = 0").is_err());
        assert!(Config::from_toml_str("context_seconds = 100000").is_err());
        assert!(Config::from_toml_str("buffer_capacity = 5").is_err());
    }

    #[test]
    fn running_requires_a_target() {
        assert!(Config::default().validate_for_run().is_err());
        let blank = Config {
            targets: vec!["  ".into()],
            ..Config::default()
        };
        assert!(blank.validate_for_run().is_err());
        let ok = Config {
            targets: vec!["a.exe".into()],
            ..Config::default()
        };
        assert!(ok.validate_for_run().is_ok());
    }

    #[test]
    fn shipped_example_is_valid_and_matches_the_defaults() {
        let example = Config::from_toml_str(include_str!("../config.example.toml")).unwrap();
        let defaults = Config::default();
        assert_eq!(example.ignore, defaults.ignore);
        assert_eq!(example.context_seconds, defaults.context_seconds);
        assert_eq!(example.buffer_capacity, defaults.buffer_capacity);
        assert!(!example.targets.is_empty());
    }

    #[test]
    fn missing_file_gives_defaults() {
        let path = temp_path("missing");
        assert_eq!(Config::load(&path).unwrap(), Config::default());
    }

    #[test]
    fn save_then_load() {
        let path = temp_path("save");
        let c = Config {
            targets: vec!["roblox".into()],
            log_file: Some(PathBuf::from("out.log")),
            ..Config::default()
        };
        c.save(&path).unwrap();
        assert_eq!(Config::load(&path).unwrap(), c);
        assert!(!path.with_extension("toml.tmp").exists());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn corrupted_file_is_an_error_not_a_silent_default() {
        let path = temp_path("corrupt");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "targets = [").unwrap();
        assert!(Config::load(&path).is_err());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
