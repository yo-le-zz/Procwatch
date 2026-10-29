//! Arguments de la ligne de commande.

use std::path::PathBuf;

use clap::Parser;
use procwatch::config::Config;

/// Surveille en temps réel le démarrage et l'arrêt des processus Windows (ETW)
/// pour comprendre pourquoi un programme s'est arrêté.
#[derive(Debug, Parser)]
#[command(name = "procwatch", version, author)]
pub struct Args {
    /// Processus à surveiller (répétez l'option ou séparez par des virgules ; `*` accepté).
    #[arg(short, long, value_delimiter = ',', value_name = "NOM")]
    pub target: Vec<String>,

    /// Processus supplémentaires à masquer (ajoutés à la liste de la configuration).
    #[arg(short, long, value_delimiter = ',', value_name = "NOM")]
    pub ignore: Vec<String>,

    /// Fichier de configuration (par défaut : %APPDATA%\procwatch\config.toml).
    #[arg(long, value_name = "FICHIER")]
    pub config: Option<PathBuf>,

    /// Ne pas afficher le menu interactif et démarrer directement.
    #[arg(long)]
    pub no_menu: bool,

    /// Écrit une copie du journal (texte brut) dans ce fichier.
    #[arg(long, value_name = "FICHIER")]
    pub log: Option<PathBuf>,

    /// Secondes d'historique affichées quand une cible se termine.
    #[arg(long, value_name = "SECONDES")]
    pub context: Option<u64>,

    /// Ne pas afficher les changements d'interfaces réseau.
    #[arg(long)]
    pub no_network: bool,

    /// Ne pas consulter le journal d'événements Windows après un plantage.
    #[arg(long)]
    pub no_event_log: bool,

    /// Afficher aussi les processus de la liste d'exclusion.
    #[arg(long)]
    pub show_ignored: bool,

    /// Désactive les couleurs (respecte aussi la variable NO_COLOR).
    #[arg(long)]
    pub no_color: bool,
}

impl Args {
    /// Superpose les options de la ligne de commande à la configuration chargée.
    pub fn apply(&self, config: &mut Config) {
        if !self.target.is_empty() {
            config.targets = self.target.clone();
        }
        for name in &self.ignore {
            if !config.ignore.contains(name) {
                config.ignore.push(name.clone());
            }
        }
        if let Some(path) = &self.log {
            config.log_file = Some(path.clone());
        }
        if let Some(seconds) = self.context {
            config.context_seconds = seconds;
        }
        if self.no_network {
            config.show_network = false;
        }
        if self.no_event_log {
            config.query_event_log = false;
        }
        if self.show_ignored {
            config.show_ignored = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Args {
        Args::try_parse_from(std::iter::once("procwatch").chain(args.iter().copied())).unwrap()
    }

    #[test]
    fn targets_replace_and_ignores_extend() {
        let mut config = Config {
            targets: vec!["old".into()],
            ..Config::default()
        };
        let defaults = config.ignore.len();
        parse(&["-t", "roblox,steam", "-i", "foo.exe"]).apply(&mut config);
        assert_eq!(config.targets, vec!["roblox", "steam"]);
        assert_eq!(config.ignore.len(), defaults + 1);
    }

    #[test]
    fn repeated_ignore_is_not_duplicated() {
        let mut config = Config::default();
        let args = parse(&["-i", "foo.exe", "-i", "foo.exe"]);
        args.apply(&mut config);
        args.apply(&mut config);
        assert_eq!(config.ignore.iter().filter(|n| *n == "foo.exe").count(), 1);
    }

    #[test]
    fn flags_override_config() {
        let mut config = Config::default();
        parse(&[
            "--no-network",
            "--no-event-log",
            "--show-ignored",
            "--context",
            "10",
            "--log",
            "a.log",
        ])
        .apply(&mut config);
        assert!(!config.show_network);
        assert!(!config.query_event_log);
        assert!(config.show_ignored);
        assert_eq!(config.context_seconds, 10);
        assert_eq!(
            config.log_file.as_deref(),
            Some(std::path::Path::new("a.log"))
        );
    }

    #[test]
    fn no_args_leaves_config_untouched() {
        let mut config = Config::default();
        parse(&[]).apply(&mut config);
        assert_eq!(config, Config::default());
    }
}
