//! Point d'entrée : arguments, menu interactif, puis boucle de surveillance.

mod cli;
mod menu;

use std::fs::OpenOptions;
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use clap::Parser;
use procwatch::config::Config;
use procwatch::event::Event;
use procwatch::exit_code::ExitKind;
use procwatch::inspector::Inspector;
use procwatch::platform::{self, EventSink};
use procwatch::render::{Line, Tone};
use procwatch::report::Report;
use procwatch::session::{CommandLineLookup, Session, SessionOptions};

use crate::cli::Args;
use crate::menu::{Outcome, UserQuit};

/// Capacité du canal entre la capture ETW et la session ; au-delà, les
/// événements sont abandonnés et comptés (voir `EventSink`).
const CHANNEL_CAPACITY: usize = 65_536;

/// Intervalle de réveil de la boucle pour tester l'arrêt demandé (Ctrl+C).
const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Délai avant de consulter le journal Windows après un plantage : le rapport
/// d'erreur d'application est écrit quelques secondes après la fin du processus.
const EVENT_LOG_DELAY: Duration = Duration::from_secs(5);

/// Marge ajoutée à la fenêtre de recherche du journal Windows.
const EVENT_LOG_MARGIN_SECONDS: u64 = 15;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if error.downcast_ref::<UserQuit>().is_some() => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("erreur : {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let args = Args::parse();

    // Vérifié avant le menu pour ne pas faire configurer l'utilisateur pour rien.
    if !platform::is_elevated() {
        if cfg!(windows) {
            bail!("droits administrateur requis : relancez procwatch dans un terminal « Exécuter en tant qu'administrateur »");
        }
        bail!("procwatch repose sur ETW et ne fonctionne que sous Windows");
    }

    let config_path = args.config.clone().or_else(Config::default_path);
    let mut config = match &config_path {
        Some(path) => Config::load(path)?,
        None => Config::default(),
    };
    args.apply(&mut config);
    config.validate()?;

    let interactive = !args.no_menu && io::stdin().is_terminal() && io::stdout().is_terminal();
    if interactive {
        match menu::run(config, config_path.as_deref())? {
            Outcome::Start(chosen) => config = chosen,
            Outcome::Quit => return Ok(()),
        }
    }
    config.validate_for_run()?;

    let color = !args.no_color
        && std::env::var_os("NO_COLOR").is_none()
        && io::stdout().is_terminal()
        && ansi_supported();
    watch(&config, color)
}

/// Active (si besoin) et teste la prise en charge des couleurs ANSI de la console.
#[cfg(windows)]
fn ansi_supported() -> bool {
    crossterm::ansi_support::supports_ansi()
}

#[cfg(not(windows))]
fn ansi_supported() -> bool {
    true
}

fn open_log(path: Option<&Path>) -> Result<Option<Box<dyn Write + Send>>> {
    let Some(path) = path else {
        return Ok(None);
    };
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("ouverture du fichier de log {}", path.display()))?;
    Ok(Some(Box::new(file)))
}

/// Consultation différée du journal Windows pour un processus qui a planté.
struct PendingLookup {
    due: Instant,
    image: String,
}

fn watch(config: &Config, color: bool) -> Result<()> {
    let stop = Arc::new(AtomicBool::new(false));
    {
        let stop = Arc::clone(&stop);
        ctrlc::set_handler(move || stop.store(true, Ordering::SeqCst))
            .context("installation du gestionnaire Ctrl+C impossible")?;
    }

    let (sender, receiver) = mpsc::sync_channel::<Event>(CHANNEL_CAPACITY);
    let dropped = Arc::new(AtomicU64::new(0));
    let sink = EventSink::new(sender, Arc::clone(&dropped));

    let mut inspector = Inspector::new()?;
    let lookup: CommandLineLookup = {
        let mut reader = Inspector::new()?;
        Box::new(move |pid| reader.command_line(pid))
    };
    let mut session = Session::new(
        SessionOptions::from_config(config, color),
        Box::new(io::stdout()),
        open_log(config.log_file.as_deref())?,
        lookup,
    );

    // La capture démarre avant l'instantané : un processus lancé entre les deux
    // est vu deux fois (sans conséquence) plutôt que jamais.
    let _trace = platform::start_process_trace(sink.clone())
        .context("démarrage de la capture ETW (droits administrateur requis)")?;
    let _network = if config.show_network {
        match platform::start_network_watch(sink.clone()) {
            Ok(guard) => Some(guard),
            Err(error) => {
                session.notice(
                    Tone::Warn,
                    "ALERTE",
                    format!("surveillance réseau indisponible : {error:#}"),
                )?;
                None
            }
        }
    } else {
        None
    };
    session.seed(&inspector.snapshot())?;

    session.notice(
        Tone::Info,
        "INFO",
        format!(
            "procwatch {} : cibles = {} ; {} processus ignorés ; contexte = {} s. Ctrl+C pour arrêter.",
            env!("CARGO_PKG_VERSION"),
            config.targets.join(", "),
            config.ignore.len(),
            config.context_seconds
        ),
    )?;

    let mut pending: Vec<PendingLookup> = Vec::new();
    while !stop.load(Ordering::SeqCst) {
        match receiver.recv_timeout(POLL_INTERVAL) {
            Ok(event) => {
                if let Some(report) = session.handle(event)? {
                    schedule_event_log(config, &report, &mut pending);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        run_due_lookups(config, &mut session, &mut pending)?;
    }

    let stats = session.stats();
    session.notice(
        Tone::Info,
        "INFO",
        format!(
            "arrêt : {} événements reçus, {} masqués, {} rapport(s), {} abandonné(s) (file saturée ou événement illisible)",
            stats.events,
            stats.ignored,
            stats.reports,
            dropped.load(Ordering::Relaxed)
        ),
    )?;
    Ok(())
}

fn schedule_event_log(config: &Config, report: &Report, pending: &mut Vec<PendingLookup>) {
    if config.query_event_log && report.verdict.kind == ExitKind::Crash {
        pending.push(PendingLookup {
            due: Instant::now() + EVENT_LOG_DELAY,
            image: report.image.clone(),
        });
    }
}

fn run_due_lookups(
    config: &Config,
    session: &mut Session,
    pending: &mut Vec<PendingLookup>,
) -> Result<()> {
    let now = Instant::now();
    let (due, waiting): (Vec<_>, Vec<_>) = pending.drain(..).partition(|p| p.due <= now);
    *pending = waiting;

    for lookup in due {
        let window = config.context_seconds + EVENT_LOG_MARGIN_SECONDS;
        let reports = platform::recent_app_errors(&lookup.image, window);
        if reports.is_empty() {
            session.notice(
                Tone::Info,
                "JOURNAL",
                format!(
                    "aucun rapport d'erreur d'application trouvé pour {}",
                    lookup.image
                ),
            )?;
            continue;
        }
        session.notice(
            Tone::Crash,
            "JOURNAL",
            format!(
                "journal d'événements Windows, {} rapport(s) pour {} :",
                reports.len(),
                lookup.image
            ),
        )?;
        for block in reports {
            for text in block.lines() {
                session.emit(&Line::new(Tone::Info, "", text).indented("| "))?;
            }
        }
    }
    Ok(())
}
