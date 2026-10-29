//! Modèle d'événements : tout ce que `procwatch` observe sur le système.

use chrono::{DateTime, Utc};

/// Identifiant de processus Windows.
pub type Pid = u32;

/// Démarrage d'un processus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessStart {
    pub pid: Pid,
    pub parent_pid: Pid,
    /// Nom de fichier de l'exécutable (sans chemin), ex. `taskkill.exe`.
    pub image: String,
    /// Ligne de commande, si elle a pu être lue à temps (best effort).
    pub command_line: Option<String>,
}

/// Fin d'un processus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessStop {
    pub pid: Pid,
    pub image: String,
    pub exit_code: u32,
}

/// Nature d'un changement d'interface réseau.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NetworkChangeKind {
    Added,
    Removed,
    Modified,
}

/// Changement d'une interface réseau (carte qui apparaît, disparaît ou change).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkChange {
    pub kind: NetworkChangeKind,
    pub interface_index: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventKind {
    Start(ProcessStart),
    Stop(ProcessStop),
    Network(NetworkChange),
}

/// Événement horodaté. L'heure est celle de réception par `procwatch`
/// (quelques centaines de microsecondes après l'événement réel).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub at: DateTime<Utc>,
    pub kind: EventKind,
}

/// Événement enrichi du contexte résolu au moment où il a été reçu.
///
/// Le nom du parent est figé ici car le parent peut avoir disparu
/// quand on affiche le rapport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub event: Event,
    pub parent_name: Option<String>,
    pub is_target: bool,
}

/// Extrait le nom de fichier d'un chemin Windows ou POSIX.
///
/// ETW peut fournir un chemin NT complet (`\Device\HarddiskVolume3\...\x.exe`)
/// ou seulement le nom : on normalise dans les deux cas.
pub fn image_basename(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_handles_nt_paths() {
        assert_eq!(
            image_basename(r"\Device\HarddiskVolume3\Windows\System32\taskkill.exe"),
            "taskkill.exe"
        );
    }

    #[test]
    fn basename_handles_plain_names_and_posix() {
        assert_eq!(image_basename("notepad.exe"), "notepad.exe");
        assert_eq!(image_basename("/usr/bin/ls"), "ls");
        assert_eq!(image_basename(""), "");
    }
}
