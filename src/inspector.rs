//! Lecture de l'état courant du système (liste des processus, ligne de commande)
//! via `sysinfo`.

use anyhow::{anyhow, Result};
use sysinfo::{Pid as SysPid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use crate::event::Pid;

/// Longueur maximale conservée pour une ligne de commande affichée.
const MAX_COMMAND_LINE_CHARS: usize = 300;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: Pid,
    pub parent_pid: Option<Pid>,
    pub name: String,
}

pub struct Inspector {
    system: System,
}

impl Inspector {
    /// Peut échouer si le système refuse l'initialisation de la lecture des processus.
    pub fn new() -> Result<Self> {
        let system = System::new()
            .map_err(|e| anyhow!("initialisation de la lecture des processus impossible : {e}"))?;
        Ok(Self { system })
    }

    /// Liste tous les processus actuellement en cours.
    pub fn snapshot(&mut self) -> Vec<ProcessInfo> {
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing(),
        );
        self.system
            .processes()
            .iter()
            .map(|(pid, process)| ProcessInfo {
                pid: pid.as_u32(),
                parent_pid: process.parent().map(SysPid::as_u32),
                name: process.name().to_string_lossy().into_owned(),
            })
            .collect()
    }

    /// Ligne de commande d'un processus, si on peut encore la lire.
    ///
    /// Retourne `None` quand le processus a déjà disparu (cas fréquent pour les
    /// outils très brefs) ou quand l'accès est refusé.
    pub fn command_line(&mut self, pid: Pid) -> Option<String> {
        let sys_pid = SysPid::from_u32(pid);
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[sys_pid]),
            true,
            ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always),
        );
        let process = self.system.process(sys_pid)?;
        let line = process
            .cmd()
            .iter()
            .map(|part| part.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ");
        (!line.is_empty()).then(|| truncate_chars(&line, MAX_COMMAND_LINE_CHARS))
    }
}

/// Tronque sur une frontière de caractère (jamais au milieu d'un caractère UTF-8).
pub fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_is_char_safe() {
        assert_eq!(truncate_chars("abc", 5), "abc");
        assert_eq!(truncate_chars("abcdef", 3), "abc…");
        assert_eq!(truncate_chars("éééé", 2), "éé…");
    }

    #[test]
    fn snapshot_contains_the_current_process() {
        let mut inspector = Inspector::new().unwrap();
        let me = std::process::id();
        assert!(inspector.snapshot().iter().any(|p| p.pid == me));
    }

    #[test]
    fn command_line_of_a_missing_process_is_none() {
        let mut inspector = Inspector::new().unwrap();
        // PID très improbable : le processus n'existe pas.
        assert_eq!(inspector.command_line(u32::MAX - 1), None);
    }
}
