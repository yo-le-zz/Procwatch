//! Accès au système : sessions ETW, réseau, journal d'événements.
//!
//! Les deux implémentations exposent exactement la même API afin que le reste
//! du programme compile (et se teste) sur toutes les plateformes.

use std::sync::atomic::AtomicU64;
use std::sync::mpsc::SyncSender;
use std::sync::Arc;

use crate::event::Event;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::{is_elevated, recent_app_errors, start_network_watch, start_process_trace};

#[cfg(not(windows))]
mod unsupported;
#[cfg(not(windows))]
pub use self::unsupported::{
    is_elevated, recent_app_errors, start_network_watch, start_process_trace,
};

/// Canal d'émission des événements vers la session.
///
/// Le canal est borné : si le consommateur ne suit plus, les événements en trop
/// sont abandonnés et comptés plutôt que d'enfler la mémoire sans limite.
#[derive(Clone)]
pub struct EventSink {
    sender: SyncSender<Event>,
    dropped: Arc<AtomicU64>,
}

impl EventSink {
    pub fn new(sender: SyncSender<Event>, dropped: Arc<AtomicU64>) -> Self {
        Self { sender, dropped }
    }

    pub fn send(&self, event: Event) {
        if self.sender.try_send(event).is_err() {
            self.count_drop();
        }
    }

    /// Compte un événement perdu (file saturée ou charge utile illisible).
    pub fn count_drop(&self) {
        self.dropped
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}
