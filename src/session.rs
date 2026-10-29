//! Cœur de la surveillance : consomme les événements, les filtre, les affiche
//! et produit un rapport quand une cible se termine.
//!
//! Cette partie ne dépend ni d'ETW ni de la console : les sorties et la lecture
//! des lignes de commande sont injectées, ce qui la rend entièrement testable.

use std::collections::HashMap;
use std::io::{self, Write};

use chrono::{DateTime, Duration, Utc};

use crate::buffer::TimedBuffer;
use crate::config::Config;
use crate::event::{
    image_basename, Event, EventKind, NetworkChange, NetworkChangeKind, Pid, ProcessStart,
    ProcessStop, Record,
};
use crate::inspector::ProcessInfo;
use crate::matcher::NameMatcher;
use crate::render::{record_line, Line, Tone};
use crate::report::Report;
use crate::table::ProcessTable;
use crate::verdict;

/// Deux notifications réseau identiques à moins de ce délai sont fusionnées :
/// une carte qui se reconfigure émet souvent des dizaines d'avis d'affilée.
const NETWORK_DEDUP: Duration = Duration::milliseconds(1_000);

/// Lecture (best effort) de la ligne de commande d'un processus qui vient de naître.
pub type CommandLineLookup = Box<dyn FnMut(Pid) -> Option<String> + Send>;

/// Compteurs affichés dans le résumé final.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Stats {
    pub events: u64,
    pub ignored: u64,
    pub reports: u64,
}

/// Réglages utiles à la session, dérivés de [`Config`].
pub struct SessionOptions {
    pub targets: NameMatcher,
    pub ignore: NameMatcher,
    pub context: Duration,
    pub buffer_capacity: usize,
    pub show_ignored: bool,
    pub show_network: bool,
    pub color: bool,
}

impl SessionOptions {
    pub fn from_config(config: &Config, color: bool) -> Self {
        Self {
            targets: NameMatcher::new(&config.targets),
            ignore: NameMatcher::new(&config.ignore),
            // `validate` borne la valeur ; la conversion ne peut donc pas déborder.
            context: Duration::seconds(i64::try_from(config.context_seconds).unwrap_or(i64::MAX)),
            buffer_capacity: config.buffer_capacity,
            show_ignored: config.show_ignored,
            show_network: config.show_network,
            color,
        }
    }
}

pub struct Session {
    opts: SessionOptions,
    table: ProcessTable,
    buffer: TimedBuffer<Record>,
    noise: TimedBuffer<String>,
    /// Cibles vivantes : PID -> heure de démarrage (inconnue si déjà lancées avant nous).
    targets_alive: HashMap<Pid, Option<DateTime<Utc>>>,
    last_network: HashMap<(NetworkChangeKind, u32), DateTime<Utc>>,
    term: Box<dyn Write + Send>,
    log: Option<Box<dyn Write + Send>>,
    lookup: CommandLineLookup,
    stats: Stats,
}

impl Session {
    pub fn new(
        opts: SessionOptions,
        term: Box<dyn Write + Send>,
        log: Option<Box<dyn Write + Send>>,
        lookup: CommandLineLookup,
    ) -> Self {
        Self {
            buffer: TimedBuffer::new(opts.context, opts.buffer_capacity),
            noise: TimedBuffer::new(opts.context, opts.buffer_capacity),
            opts,
            table: ProcessTable::new(),
            targets_alive: HashMap::new(),
            last_network: HashMap::new(),
            term,
            log,
            lookup,
            stats: Stats::default(),
        }
    }

    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// Affiche un message d'information (hors flux d'événements).
    pub fn notice(&mut self, tone: Tone, tag: &str, message: impl Into<String>) -> io::Result<()> {
        self.emit(&Line::new(tone, tag, message))
    }

    /// Enregistre l'état initial : noms de processus pour résoudre les parents,
    /// et cibles déjà en cours d'exécution.
    pub fn seed(&mut self, processes: &[ProcessInfo]) -> io::Result<()> {
        for p in processes {
            self.table.on_start(p.pid, &p.name);
        }
        for p in processes {
            if self.opts.targets.is_match(&p.name) {
                self.targets_alive.insert(p.pid, None);
                self.notice(
                    Tone::Info,
                    "INFO",
                    format!("cible déjà en cours : {} (pid {})", p.name, p.pid),
                )?;
            }
        }
        Ok(())
    }

    /// Traite un événement. Renvoie un rapport si une cible vient de se terminer.
    pub fn handle(&mut self, event: Event) -> io::Result<Option<Report>> {
        self.stats.events += 1;
        match event.kind {
            EventKind::Start(start) => self.on_start(event.at, start).map(|()| None),
            EventKind::Stop(stop) => self.on_stop(event.at, stop),
            EventKind::Network(change) => self.on_network(event.at, change).map(|()| None),
        }
    }

    fn is_hidden(&self, image: &str, is_target: bool) -> bool {
        !is_target && !self.opts.show_ignored && self.opts.ignore.is_match(image)
    }

    fn on_start(&mut self, at: DateTime<Utc>, mut start: ProcessStart) -> io::Result<()> {
        start.image = image_basename(&start.image).to_owned();
        let is_target = self.opts.targets.is_match(&start.image);
        let parent_name = self.table.name_of(start.parent_pid).map(str::to_owned);
        self.table.on_start(start.pid, &start.image);

        if self.is_hidden(&start.image, is_target) {
            self.stats.ignored += 1;
            self.noise.push(at, start.image);
            return Ok(());
        }
        if start.command_line.is_none() {
            start.command_line = (self.lookup)(start.pid);
        }
        if is_target {
            self.targets_alive.insert(start.pid, Some(at));
        }
        let record = Record {
            event: Event {
                at,
                kind: EventKind::Start(start),
            },
            parent_name,
            is_target,
        };
        self.emit(&record_line(&record))?;
        self.buffer.push(at, record);
        Ok(())
    }

    fn on_stop(&mut self, at: DateTime<Utc>, mut stop: ProcessStop) -> io::Result<Option<Report>> {
        let known_name = self.table.on_stop(stop.pid);
        let reported = image_basename(&stop.image);
        stop.image = if reported.is_empty() {
            known_name.unwrap_or_else(|| "?".to_owned())
        } else {
            reported.to_owned()
        };

        let tracked = self.targets_alive.remove(&stop.pid);
        let is_target = tracked.is_some() || self.opts.targets.is_match(&stop.image);

        if self.is_hidden(&stop.image, is_target) {
            self.stats.ignored += 1;
            self.noise.push(at, stop.image);
            return Ok(None);
        }

        // Le contexte est figé avant d'ajouter l'arrêt lui-même.
        let report = is_target.then(|| self.build_report(at, &stop, tracked.flatten()));

        let record = Record {
            event: Event {
                at,
                kind: EventKind::Stop(stop),
            },
            parent_name: None,
            is_target,
        };
        self.emit(&record_line(&record))?;
        self.buffer.push(at, record);

        if let Some(report) = &report {
            self.stats.reports += 1;
            for line in report.lines() {
                self.emit(&line)?;
            }
        }
        Ok(report)
    }

    fn build_report(
        &self,
        at: DateTime<Utc>,
        stop: &ProcessStop,
        started: Option<DateTime<Utc>>,
    ) -> Report {
        let window = self.opts.context;
        let context: Vec<Record> = self.buffer.recent(at, window).cloned().collect();
        let refs: Vec<&Record> = context.iter().collect();
        let verdict = verdict::analyze(stop.pid, &stop.image, stop.exit_code, &refs);

        let mut counts: HashMap<&str, u64> = HashMap::new();
        for name in self.noise.recent(at, window) {
            *counts.entry(name.as_str()).or_default() += 1;
        }
        let mut ignored: Vec<(String, u64)> =
            counts.into_iter().map(|(n, c)| (n.to_owned(), c)).collect();
        ignored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

        Report {
            image: stop.image.clone(),
            pid: stop.pid,
            lived: started.map(|s| at - s),
            verdict,
            context,
            ignored,
            window_seconds: window.num_seconds(),
        }
    }

    fn on_network(&mut self, at: DateTime<Utc>, change: NetworkChange) -> io::Result<()> {
        if !self.opts.show_network {
            return Ok(());
        }
        let key = (change.kind, change.interface_index);
        if let Some(previous) = self.last_network.get(&key) {
            if at - *previous < NETWORK_DEDUP {
                return Ok(());
            }
        }
        self.last_network.insert(key, at);
        let record = Record {
            event: Event {
                at,
                kind: EventKind::Network(change),
            },
            parent_name: None,
            is_target: false,
        };
        self.emit(&record_line(&record))?;
        self.buffer.push(at, record);
        Ok(())
    }

    /// Écrit une ligne dans le terminal (colorée ou non) et dans le fichier de log.
    pub fn emit(&mut self, line: &Line) -> io::Result<()> {
        let text = if self.opts.color {
            line.ansi()
        } else {
            line.plain()
        };
        writeln!(self.term, "{text}")?;
        self.term.flush()?;

        let log_error = self.log.as_mut().and_then(|log| {
            writeln!(log, "{}", line.plain())
                .and_then(|()| log.flush())
                .err()
        });
        if let Some(error) = log_error {
            // Un journal fichier défaillant ne doit pas interrompre la surveillance.
            self.log = None;
            let warning = Line::new(
                Tone::Warn,
                "ALERTE",
                format!(
                    "écriture du fichier de log impossible ({error}) : journal fichier désactivé"
                ),
            );
            writeln!(
                self.term,
                "{}",
                if self.opts.color {
                    warning.ansi()
                } else {
                    warning.plain()
                }
            )?;
        }
        Ok(())
    }
}
