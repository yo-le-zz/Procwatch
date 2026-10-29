//! Mise en forme des lignes affichées : version colorée (terminal) et version
//! texte brut (fichier de log). Le texte reste en ASCII pour les symboles afin
//! de s'afficher correctement dans n'importe quelle console Windows.

use chrono::{DateTime, Duration, Local, Utc};
use crossterm::style::{Color, Stylize};

use crate::event::{EventKind, NetworkChangeKind, Record};
use crate::exit_code::{self, ExitKind};
use crate::inspector::truncate_chars;

/// Longueur maximale d'une ligne de commande dans une ligne d'événement.
const MAX_INLINE_COMMAND_CHARS: usize = 200;

/// Largeur de la colonne d'étiquette (`START`, `CIBLE<`…).
const TAG_WIDTH: usize = 7;

/// Famille de couleur d'une ligne.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Start,
    Target,
    Clean,
    Error,
    Crash,
    Network,
    Info,
    Warn,
}

impl Tone {
    fn color(self) -> Color {
        match self {
            Tone::Start => Color::Green,
            Tone::Target => Color::Magenta,
            Tone::Clean => Color::Cyan,
            Tone::Error | Tone::Warn => Color::Yellow,
            Tone::Crash => Color::Red,
            Tone::Network => Color::Blue,
            Tone::Info => Color::DarkGrey,
        }
    }

    pub fn from_exit(kind: ExitKind) -> Self {
        match kind {
            ExitKind::Clean | ExitKind::CtrlC => Tone::Clean,
            ExitKind::Error => Tone::Error,
            ExitKind::Crash => Tone::Crash,
        }
    }
}

/// Une ligne prête à être affichée.
#[derive(Debug, Clone)]
pub struct Line {
    pub tone: Tone,
    pub stamp: Option<String>,
    pub tag: String,
    pub body: String,
    /// Corps coloré et en gras (cibles et plantages).
    pub emphasis: bool,
    /// Préfixe de retrait (lignes de contexte d'un rapport).
    pub prefix: &'static str,
}

impl Line {
    pub fn new(tone: Tone, tag: &str, body: impl Into<String>) -> Self {
        Self {
            tone,
            stamp: None,
            tag: tag.to_owned(),
            body: body.into(),
            emphasis: false,
            prefix: "",
        }
    }

    pub fn with_stamp(mut self, at: DateTime<Utc>) -> Self {
        self.stamp = Some(stamp(at));
        self
    }

    pub fn emphasized(mut self) -> Self {
        self.emphasis = true;
        self
    }

    pub fn indented(mut self, prefix: &'static str) -> Self {
        self.prefix = prefix;
        self
    }

    /// Version sans aucune séquence d'échappement (log, tests).
    pub fn plain(&self) -> String {
        let mut parts = Vec::with_capacity(3);
        if let Some(s) = &self.stamp {
            parts.push(s.clone());
        }
        if !self.tag.is_empty() {
            parts.push(format!("{:<TAG_WIDTH$}", self.tag));
        }
        parts.push(self.body.clone());
        format!("{}{}", self.prefix, parts.join("  "))
    }

    /// Version colorée pour le terminal.
    pub fn ansi(&self) -> String {
        let color = self.tone.color();
        let mut parts = Vec::with_capacity(3);
        if let Some(s) = &self.stamp {
            parts.push(s.as_str().dark_grey().to_string());
        }
        if !self.tag.is_empty() {
            parts.push(
                format!("{:<TAG_WIDTH$}", self.tag)
                    .with(color)
                    .bold()
                    .to_string(),
            );
        }
        parts.push(if self.emphasis {
            self.body.as_str().with(color).bold().to_string()
        } else {
            self.body.clone()
        });
        format!("{}{}", self.prefix, parts.join("  "))
    }
}

/// Heure locale avec millisecondes : `20:00:17.542`.
pub fn stamp(at: DateTime<Utc>) -> String {
    at.with_timezone(&Local).format("%H:%M:%S%.3f").to_string()
}

/// Durée lisible : `850 ms`, `5.3 s`, `2 min 03 s`, `1 h 02 min`.
pub fn format_duration(d: Duration) -> String {
    let ms = d.num_milliseconds().max(0);
    if ms < 1_000 {
        return format!("{ms} ms");
    }
    let secs = ms / 1_000;
    if secs < 60 {
        return format!("{}.{} s", secs, (ms % 1_000) / 100);
    }
    let mins = secs / 60;
    if mins < 60 {
        return format!("{mins} min {:02} s", secs % 60);
    }
    format!("{} h {:02} min", mins / 60, mins % 60)
}

/// Ligne d'affichage d'un événement enregistré.
pub fn record_line(record: &Record) -> Line {
    let at = record.event.at;
    match &record.event.kind {
        EventKind::Start(start) => {
            let parent = record.parent_name.as_deref().unwrap_or("?");
            let mut body = format!(
                "{} (pid {})  parent={}({})",
                start.image, start.pid, parent, start.parent_pid
            );
            if let Some(cmd) = &start.command_line {
                body.push_str("  ");
                body.push_str(&truncate_chars(cmd, MAX_INLINE_COMMAND_CHARS));
            }
            if record.is_target {
                Line::new(Tone::Target, "CIBLE>", body)
                    .with_stamp(at)
                    .emphasized()
            } else {
                Line::new(Tone::Start, "START", body).with_stamp(at)
            }
        }
        EventKind::Stop(stop) => {
            let kind = exit_code::classify(stop.exit_code);
            let body = format!(
                "{} (pid {})  {}",
                stop.image,
                stop.pid,
                exit_code::describe(stop.exit_code)
            );
            let tag = if record.is_target { "CIBLE<" } else { "STOP" };
            let line = Line::new(Tone::from_exit(kind), tag, body).with_stamp(at);
            if record.is_target || kind == ExitKind::Crash {
                line.emphasized()
            } else {
                line
            }
        }
        EventKind::Network(change) => {
            let what = match change.kind {
                NetworkChangeKind::Added => "ajoutée",
                NetworkChangeKind::Removed => "supprimée",
                NetworkChangeKind::Modified => "modifiée",
            };
            Line::new(
                Tone::Network,
                "NET",
                format!("interface réseau #{} {what}", change.interface_index),
            )
            .with_stamp(at)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{Event, NetworkChange, ProcessStart, ProcessStop};
    use chrono::TimeZone;

    fn record(kind: EventKind, is_target: bool) -> Record {
        Record {
            event: Event {
                at: Utc.timestamp_opt(1_700_000_000, 0).unwrap(),
                kind,
            },
            parent_name: Some("explorer.exe".into()),
            is_target,
        }
    }

    #[test]
    fn durations_are_readable() {
        assert_eq!(format_duration(Duration::milliseconds(850)), "850 ms");
        assert_eq!(format_duration(Duration::milliseconds(5_300)), "5.3 s");
        assert_eq!(format_duration(Duration::seconds(123)), "2 min 03 s");
        assert_eq!(format_duration(Duration::seconds(3_720)), "1 h 02 min");
        assert_eq!(format_duration(Duration::milliseconds(-5)), "0 ms");
    }

    #[test]
    fn start_line_shows_parent_and_command() {
        let r = record(
            EventKind::Start(ProcessStart {
                pid: 7,
                parent_pid: 3,
                image: "taskkill.exe".into(),
                command_line: Some("taskkill /F /PID 4432".into()),
            }),
            false,
        );
        let text = record_line(&r).plain();
        assert!(text.contains("START"));
        assert!(text.contains("taskkill.exe (pid 7)"));
        assert!(text.contains("parent=explorer.exe(3)"));
        assert!(text.contains("taskkill /F /PID 4432"));
    }

    #[test]
    fn target_lines_use_the_target_tags() {
        let start = record(
            EventKind::Start(ProcessStart {
                pid: 1,
                parent_pid: 2,
                image: "a.exe".into(),
                command_line: None,
            }),
            true,
        );
        let stop = record(
            EventKind::Stop(ProcessStop {
                pid: 1,
                image: "a.exe".into(),
                exit_code: 0xC000_0005,
            }),
            true,
        );
        assert!(record_line(&start).plain().contains("CIBLE>"));
        let stop_line = record_line(&stop);
        assert!(stop_line.plain().contains("CIBLE<"));
        assert_eq!(stop_line.tone, Tone::Crash);
        assert!(stop_line.emphasis);
    }

    #[test]
    fn stop_tone_follows_exit_kind() {
        let mk = |code| {
            record_line(&record(
                EventKind::Stop(ProcessStop {
                    pid: 1,
                    image: "a.exe".into(),
                    exit_code: code,
                }),
                false,
            ))
            .tone
        };
        assert_eq!(mk(0), Tone::Clean);
        assert_eq!(mk(1), Tone::Error);
        assert_eq!(mk(0xC000_0005), Tone::Crash);
    }

    #[test]
    fn network_line() {
        let r = record(
            EventKind::Network(NetworkChange {
                kind: NetworkChangeKind::Removed,
                interface_index: 12,
            }),
            false,
        );
        assert!(record_line(&r)
            .plain()
            .contains("interface réseau #12 supprimée"));
    }

    #[test]
    fn plain_has_no_escape_sequences_and_ansi_has() {
        let line = Line::new(Tone::Crash, "STOP", "x").emphasized();
        assert!(!line.plain().contains('\u{1b}'));
        assert!(line.ansi().contains('\u{1b}'));
    }

    #[test]
    fn empty_tag_and_prefix() {
        let line = Line::new(Tone::Info, "", "texte").indented("| ");
        assert_eq!(line.plain(), "| texte");
    }
}
