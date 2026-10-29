//! Analyse de fin de processus : plantage, arrêt forcé ou sortie normale ?
//!
//! Limite assumée : ETW ne fournit pas l'identité de celui qui appelle
//! `TerminateProcess`. On ne peut donc désigner un « coupable » que si un outil
//! d'arrêt (taskkill, PowerShell…) a été lancé avant et cite le PID ou le nom
//! de la cible dans sa ligne de commande. Un arrêt direct depuis le Gestionnaire
//! des tâches, par exemple, apparaîtra comme « code 1 » sans coupable.

use crate::event::{EventKind, Pid, Record};
use crate::exit_code::{self, ExitKind};

/// Outils qui servent couramment à tuer un processus.
const KILLER_TOOLS: &[&str] = &[
    "taskkill.exe",
    "pskill.exe",
    "pskill64.exe",
    "wmic.exe",
    "powershell.exe",
    "pwsh.exe",
    "ntsd.exe",
];

/// Longueur minimale d'un nom pour être cherché dans une ligne de commande
/// (évite les faux positifs avec des noms d'une ou deux lettres).
const MIN_NAME_LEN: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suspect {
    pub name: String,
    pub pid: Pid,
    pub parent_name: Option<String>,
    pub command_line: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    pub kind: ExitKind,
    pub exit_code: u32,
    pub killer: Option<Suspect>,
}

fn mentions_target(command_line: &str, pid: Pid, image: &str) -> bool {
    let lower = command_line.to_lowercase();
    let pid_text = pid.to_string();
    if lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|token| token == pid_text)
    {
        return true;
    }
    let stem = image.to_lowercase();
    let stem = stem.strip_suffix(".exe").unwrap_or(&stem);
    stem.len() >= MIN_NAME_LEN && lower.contains(stem)
}

/// Cherche, parmi les événements récents, l'outil d'arrêt qui visait la cible.
/// On retient le plus récent : c'est le plus proche de la mort du processus.
fn find_killer(window: &[&Record], pid: Pid, image: &str) -> Option<Suspect> {
    window.iter().rev().find_map(|record| {
        let EventKind::Start(start) = &record.event.kind else {
            return None;
        };
        let is_tool = KILLER_TOOLS
            .iter()
            .any(|tool| start.image.eq_ignore_ascii_case(tool));
        let line = start.command_line.as_deref()?;
        (is_tool && mentions_target(line, pid, image)).then(|| Suspect {
            name: start.image.clone(),
            pid: start.pid,
            parent_name: record.parent_name.clone(),
            command_line: Some(line.to_owned()),
        })
    })
}

/// Construit le verdict d'un processus cible terminé.
///
/// `window` contient les événements qui ont précédé la fin, du plus ancien
/// au plus récent.
pub fn analyze(pid: Pid, image: &str, exit_code: u32, window: &[&Record]) -> Verdict {
    Verdict {
        kind: exit_code::classify(exit_code),
        exit_code,
        killer: find_killer(window, pid, image),
    }
}

impl Verdict {
    /// Phrase de synthèse affichée en tête du rapport.
    pub fn headline(&self) -> String {
        let code = exit_code::describe(self.exit_code);
        let by = |s: &Suspect| match &s.parent_name {
            Some(parent) => format!("{} (pid {}, lancé par {})", s.name, s.pid, parent),
            None => format!("{} (pid {})", s.name, s.pid),
        };
        match (self.kind, &self.killer) {
            (ExitKind::Crash, Some(s)) => format!(
                "PLANTAGE : {code}. À noter : {} visait ce processus juste avant.",
                by(s)
            ),
            (ExitKind::Crash, None) => format!("PLANTAGE : {code}"),
            (ExitKind::Clean, Some(s)) => {
                format!("Fermeture demandée par {} : {code}", by(s))
            }
            (ExitKind::Clean, None) => format!("Sortie normale : {code}"),
            (ExitKind::CtrlC, _) => format!("Interrompu par Ctrl+C : {code}"),
            (ExitKind::Error, Some(s)) => {
                format!("ARRÊT FORCÉ par {} : {code}", by(s))
            }
            (ExitKind::Error, None) => {
                format!("Fin anormale sans plantage identifié : {code}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{Event, ProcessStart};
    use chrono::{TimeZone, Utc};

    fn start_record(image: &str, pid: Pid, parent: Option<&str>, cmd: Option<&str>) -> Record {
        Record {
            event: Event {
                at: Utc.timestamp_opt(1_700_000_000, 0).unwrap(),
                kind: EventKind::Start(ProcessStart {
                    pid,
                    parent_pid: 1,
                    image: image.to_owned(),
                    command_line: cmd.map(str::to_owned),
                }),
            },
            parent_name: parent.map(str::to_owned),
            is_target: false,
        }
    }

    #[test]
    fn taskkill_targeting_the_pid_is_the_killer() {
        let r = start_record(
            "taskkill.exe",
            20112,
            Some("Fleasion.exe"),
            Some("taskkill /F /PID 4432"),
        );
        let v = analyze(4432, "RobloxPlayerBeta.exe", 1, &[&r]);
        assert_eq!(v.kind, ExitKind::Error);
        let killer = v.killer.as_ref().expect("coupable attendu");
        assert_eq!(killer.name, "taskkill.exe");
        assert_eq!(killer.parent_name.as_deref(), Some("Fleasion.exe"));
        assert!(v.headline().contains("ARRÊT FORCÉ par taskkill.exe"));
        assert!(v.headline().contains("lancé par Fleasion.exe"));
    }

    #[test]
    fn taskkill_by_image_name_is_detected() {
        let r = start_record(
            "taskkill.exe",
            5,
            None,
            Some("taskkill /IM robloxplayerbeta.exe /F"),
        );
        let v = analyze(4432, "RobloxPlayerBeta.exe", 1, &[&r]);
        assert!(v.killer.is_some());
    }

    #[test]
    fn pid_must_match_as_a_whole_token() {
        let r = start_record("taskkill.exe", 5, None, Some("taskkill /F /PID 44321"));
        let v = analyze(4432, "app.exe", 1, &[&r]);
        assert!(v.killer.is_none());
    }

    #[test]
    fn unrelated_taskkill_is_ignored() {
        let r = start_record("taskkill.exe", 5, None, Some("taskkill /F /PID 999"));
        assert!(analyze(4432, "RobloxPlayerBeta.exe", 1, &[&r])
            .killer
            .is_none());
    }

    #[test]
    fn non_killer_tool_is_ignored_even_if_it_cites_the_pid() {
        let r = start_record("notepad.exe", 5, None, Some("notepad 4432"));
        assert!(analyze(4432, "RobloxPlayerBeta.exe", 1, &[&r])
            .killer
            .is_none());
    }

    #[test]
    fn missing_command_line_gives_no_killer() {
        let r = start_record("taskkill.exe", 5, None, None);
        assert!(analyze(4432, "x.exe", 1, &[&r]).killer.is_none());
    }

    #[test]
    fn most_recent_killer_wins() {
        let a = start_record("taskkill.exe", 1, None, Some("taskkill /PID 4432"));
        let b = start_record("pskill.exe", 2, None, Some("pskill 4432"));
        let v = analyze(4432, "x.exe", 1, &[&a, &b]);
        assert_eq!(v.killer.unwrap().name, "pskill.exe");
    }

    #[test]
    fn crash_is_reported_as_crash_even_with_a_killer() {
        let r = start_record("taskkill.exe", 5, None, Some("taskkill /PID 4432"));
        let v = analyze(4432, "x.exe", 0xC000_0005, &[&r]);
        assert_eq!(v.kind, ExitKind::Crash);
        assert!(v.headline().starts_with("PLANTAGE"));
        assert!(v.headline().contains("taskkill.exe"));
    }

    #[test]
    fn clean_exit_without_killer() {
        let v = analyze(1, "x.exe", 0, &[]);
        assert_eq!(v.kind, ExitKind::Clean);
        assert!(v.headline().starts_with("Sortie normale"));
    }

    #[test]
    fn error_without_killer_is_not_called_a_crash() {
        let v = analyze(1, "x.exe", 1, &[]);
        assert!(v.headline().contains("sans plantage identifié"));
    }
}
