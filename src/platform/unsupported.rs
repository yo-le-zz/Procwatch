//! Implémentation de repli : `procwatch` repose sur ETW, propre à Windows.

use anyhow::{bail, Result};

use super::EventSink;

/// Garde de session ETW (jamais construite sur cette plateforme).
pub struct TraceGuard;

/// Garde de surveillance réseau (jamais construite sur cette plateforme).
pub struct NetworkGuard;

pub fn is_elevated() -> bool {
    false
}

pub fn start_process_trace(_sink: EventSink) -> Result<TraceGuard> {
    bail!("procwatch repose sur ETW et ne fonctionne que sous Windows")
}

pub fn start_network_watch(_sink: EventSink) -> Result<NetworkGuard> {
    bail!("la surveillance réseau n'est disponible que sous Windows")
}

pub fn recent_app_errors(_image: &str, _within_seconds: u64) -> Vec<String> {
    Vec::new()
}
