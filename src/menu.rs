//! Menu interactif de configuration (inquire).

use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::Result;
use inquire::{CustomType, InquireError, MultiSelect, Select, Text};
use procwatch::config::{Config, DEFAULT_IGNORE, MAX_CONTEXT_SECONDS, MIN_CONTEXT_SECONDS};
use procwatch::inspector::Inspector;
use procwatch::matcher::split_list;

/// Résultat du menu principal.
pub enum Outcome {
    Start(Config),
    Quit,
}

/// L'utilisateur a quitté avec Ctrl+C pendant une question.
#[derive(Debug)]
pub struct UserQuit;

impl fmt::Display for UserQuit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "interrompu par l'utilisateur")
    }
}

impl std::error::Error for UserQuit {}

/// Traduit le résultat d'une question : Échap = retour (`None`), Ctrl+C = quitter.
fn ask<T>(answer: Result<T, InquireError>) -> Result<Option<T>> {
    match answer {
        Ok(value) => Ok(Some(value)),
        Err(InquireError::OperationCanceled) => Ok(None),
        Err(InquireError::OperationInterrupted) => Err(UserQuit.into()),
        Err(other) => Err(other.into()),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Main {
    Start,
    Targets,
    Ignore,
    Options,
    Save,
    Quit,
}

impl fmt::Display for Main {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Main::Start => "Démarrer la surveillance",
            Main::Targets => "Définir les processus à surveiller (cibles)",
            Main::Ignore => "Définir les processus à ignorer",
            Main::Options => "Options",
            Main::Save => "Enregistrer la configuration",
            Main::Quit => "Quitter",
        })
    }
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "oui"
    } else {
        "non"
    }
}

fn summary(config: &Config) {
    println!();
    println!(
        "  Cibles   : {}",
        if config.targets.is_empty() {
            "(aucune)".to_owned()
        } else {
            config.targets.join(", ")
        }
    );
    println!("  Ignorés  : {} processus", config.ignore.len());
    println!(
        "  Contexte : {} s avant la fin d'une cible",
        config.context_seconds
    );
    println!();
}

pub fn run(mut config: Config, config_path: Option<&Path>) -> Result<Outcome> {
    loop {
        summary(&config);
        let entries = vec![
            Main::Start,
            Main::Targets,
            Main::Ignore,
            Main::Options,
            Main::Save,
            Main::Quit,
        ];
        let prompt = Select::new("procwatch : menu principal", entries)
            .with_help_message("↑↓ pour naviguer, Entrée pour valider, Échap pour quitter")
            .prompt();
        let Some(choice) = ask(prompt)? else {
            return Ok(Outcome::Quit);
        };
        match choice {
            Main::Start => match config.validate_for_run() {
                Ok(()) => return Ok(Outcome::Start(config)),
                Err(e) => println!("  ! {e:#}"),
            },
            Main::Targets => {
                if let Some(list) = edit_list("Cibles", &config.targets, None)? {
                    config.targets = list;
                }
            }
            Main::Ignore => {
                let defaults: Vec<String> =
                    DEFAULT_IGNORE.iter().map(|s| (*s).to_owned()).collect();
                if let Some(list) = edit_list("Processus ignorés", &config.ignore, Some(&defaults))?
                {
                    config.ignore = list;
                }
            }
            Main::Options => edit_options(&mut config)?,
            Main::Save => save(&config, config_path),
            Main::Quit => return Ok(Outcome::Quit),
        }
    }
}

fn save(config: &Config, path: Option<&Path>) {
    match path {
        None => println!("  ! aucun emplacement de configuration disponible (utilisez --config)"),
        Some(path) => match config.save(path) {
            Ok(()) => println!("  Configuration enregistrée dans {}", path.display()),
            Err(e) => println!("  ! {e:#}"),
        },
    }
}

/// Sous-menu d'édition d'une liste de noms. `None` = pas de changement.
fn edit_list(
    title: &str,
    current: &[String],
    defaults: Option<&[String]>,
) -> Result<Option<Vec<String>>> {
    let mut entries = vec![
        "Saisir les noms à la main".to_owned(),
        "Choisir parmi les processus en cours".to_owned(),
    ];
    if defaults.is_some() {
        entries.push("Rétablir la liste par défaut".to_owned());
    }
    entries.push("Retour".to_owned());

    let Some(choice) =
        ask(Select::new(&format!("{title} : comment les définir ?"), entries).raw_prompt())?
    else {
        return Ok(None);
    };
    match (choice.index, defaults) {
        (0, _) => {
            let help = "Séparés par des virgules ; * accepté (ex. robloxplayerbeta, steam*)";
            let initial = current.join(", ");
            let answer = Text::new(&format!("{title} :"))
                .with_initial_value(&initial)
                .with_help_message(help)
                .prompt();
            Ok(ask(answer)?.map(|text| split_list(&text)))
        }
        (1, _) => pick_running(title, current),
        (2, Some(defaults)) => Ok(Some(defaults.to_vec())),
        _ => Ok(None),
    }
}

/// Sélection multiple parmi les processus actuellement en cours.
fn pick_running(title: &str, current: &[String]) -> Result<Option<Vec<String>>> {
    let mut inspector = Inspector::new()?;
    let mut names: Vec<String> = inspector.snapshot().into_iter().map(|p| p.name).collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));

    let is_listed = |entry: &String| names.iter().any(|n| n.eq_ignore_ascii_case(entry));
    let defaults: Vec<usize> = names
        .iter()
        .enumerate()
        .filter(|(_, n)| current.iter().any(|c| c.eq_ignore_ascii_case(n)))
        .map(|(i, _)| i)
        .collect();

    let answer = MultiSelect::new(
        &format!("{title} : cochez avec Espace, tapez pour filtrer"),
        names.clone(),
    )
    .with_default(&defaults)
    .with_page_size(15)
    .prompt();
    let Some(picked) = ask(answer)? else {
        return Ok(None);
    };
    // Les motifs saisis à la main (jokers, processus absents) sont conservés.
    let mut result: Vec<String> = current.iter().filter(|c| !is_listed(c)).cloned().collect();
    result.extend(picked);
    Ok(Some(result))
}

fn edit_options(config: &mut Config) -> Result<()> {
    loop {
        let labels = vec![
            format!(
                "Historique avant la fin d'une cible : {} s",
                config.context_seconds
            ),
            format!(
                "Afficher les changements réseau : {}",
                yes_no(config.show_network)
            ),
            format!(
                "Consulter le journal d'événements Windows après un plantage : {}",
                yes_no(config.query_event_log)
            ),
            format!(
                "Afficher aussi les processus ignorés : {}",
                yes_no(config.show_ignored)
            ),
            format!(
                "Fichier de log : {}",
                config
                    .log_file
                    .as_deref()
                    .map_or("aucun".to_owned(), |p| p.display().to_string())
            ),
            "Retour".to_owned(),
        ];
        let Some(choice) = ask(Select::new("Options", labels).raw_prompt())? else {
            return Ok(());
        };
        match choice.index {
            0 => {
                let answer = CustomType::<u64>::new("Nombre de secondes :")
                    .with_default(config.context_seconds)
                    .with_error_message("Entrez un nombre entier")
                    .prompt();
                if let Some(seconds) = ask(answer)? {
                    if (MIN_CONTEXT_SECONDS..=MAX_CONTEXT_SECONDS).contains(&seconds) {
                        config.context_seconds = seconds;
                    } else {
                        println!("  ! valeur entre {MIN_CONTEXT_SECONDS} et {MAX_CONTEXT_SECONDS} attendue");
                    }
                }
            }
            1 => config.show_network = !config.show_network,
            2 => config.query_event_log = !config.query_event_log,
            3 => config.show_ignored = !config.show_ignored,
            4 => {
                let initial = config
                    .log_file
                    .as_deref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                let answer = Text::new("Chemin du fichier (vide = aucun) :")
                    .with_initial_value(&initial)
                    .prompt();
                if let Some(text) = ask(answer)? {
                    let text = text.trim();
                    config.log_file = (!text.is_empty()).then(|| PathBuf::from(text));
                }
            }
            _ => return Ok(()),
        }
    }
}
