//! Table des processus connus : permet de retrouver le nom d'un parent à
//! partir de son PID, même si ce parent s'est terminé entre-temps.

use std::collections::{HashMap, VecDeque};

use crate::event::Pid;

/// Nombre de processus terminés dont on garde le nom (évite une croissance infinie).
const EXITED_MEMORY: usize = 2048;

#[derive(Debug, Default)]
pub struct ProcessTable {
    live: HashMap<Pid, String>,
    exited: HashMap<Pid, String>,
    exited_order: VecDeque<Pid>,
}

impl ProcessTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Enregistre un processus vivant (démarrage ou état initial).
    pub fn on_start(&mut self, pid: Pid, name: &str) {
        self.live.insert(pid, name.to_owned());
        // Un PID réutilisé ne doit plus être résolu vers l'ancien processus terminé.
        self.exited.remove(&pid);
    }

    /// Retire un processus terminé et mémorise son nom pour les enfants tardifs.
    /// Renvoie le nom connu, s'il y en a un.
    pub fn on_stop(&mut self, pid: Pid) -> Option<String> {
        let name = self.live.remove(&pid)?;
        if self.exited.insert(pid, name.clone()).is_none() {
            self.exited_order.push_back(pid);
        }
        while self.exited_order.len() > EXITED_MEMORY {
            if let Some(old) = self.exited_order.pop_front() {
                self.exited.remove(&old);
            }
        }
        Some(name)
    }

    /// Nom d'un processus : vivant en priorité, sinon récemment terminé.
    pub fn name_of(&self, pid: Pid) -> Option<&str> {
        self.live
            .get(&pid)
            .or_else(|| self.exited.get(&pid))
            .map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_live_and_exited_processes() {
        let mut t = ProcessTable::new();
        t.on_start(10, "parent.exe");
        assert_eq!(t.name_of(10), Some("parent.exe"));
        assert_eq!(t.on_stop(10).as_deref(), Some("parent.exe"));
        assert_eq!(t.name_of(10), Some("parent.exe"));
        assert_eq!(t.name_of(99), None);
    }

    #[test]
    fn pid_reuse_prefers_the_new_process() {
        let mut t = ProcessTable::new();
        t.on_start(10, "old.exe");
        t.on_stop(10);
        t.on_start(10, "new.exe");
        assert_eq!(t.name_of(10), Some("new.exe"));
    }

    #[test]
    fn stopping_unknown_pid_is_harmless() {
        let mut t = ProcessTable::new();
        assert_eq!(t.on_stop(5), None);
    }

    #[test]
    fn exited_memory_is_bounded() {
        let mut t = ProcessTable::new();
        for pid in 0..(EXITED_MEMORY as u32 + 100) {
            t.on_start(pid, "x.exe");
            t.on_stop(pid);
        }
        assert_eq!(t.name_of(0), None);
        assert!(t.name_of(EXITED_MEMORY as u32 + 99).is_some());
    }
}
