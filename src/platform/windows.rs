//! Implémentation Windows : session ETW sur `Microsoft-Windows-Kernel-Process`,
//! notifications de changement d'interface réseau et lecture du journal
//! d'événements.

use std::ffi::c_void;
use std::mem::size_of;
use std::process::Command;
use std::sync::OnceLock;

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use ferrisetw::parser::Parser;
use ferrisetw::provider::Provider;
use ferrisetw::schema_locator::SchemaLocator;
use ferrisetw::trace::UserTrace;
use ferrisetw::EventRecord;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::NetworkManagement::IpHelper::{
    CancelMibChangeNotify2, NotifyIpInterfaceChange, MIB_IPINTERFACE_ROW, MIB_NOTIFICATION_TYPE,
};
use windows::Win32::Networking::WinSock::AF_UNSPEC;
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

use super::EventSink;
use crate::event::{Event, EventKind, NetworkChange, NetworkChangeKind, ProcessStart, ProcessStop};

/// GUID du fournisseur `Microsoft-Windows-Kernel-Process`.
const KERNEL_PROCESS_GUID: &str = "22fb2cd6-0e7b-422b-a0c7-2fad1fd0e716";

/// Mot-clé `WINEVENT_KEYWORD_PROCESS` : démarrages et arrêts uniquement,
/// sans le flot de chargements d'images ni de threads.
const KEYWORD_PROCESS: u64 = 0x10;

/// Identifiants d'événements du fournisseur.
const EVENT_PROCESS_START: u16 = 1;
const EVENT_PROCESS_STOP: u16 = 2;

/// Nombre maximal de rapports du journal Windows affichés.
const MAX_APP_ERROR_REPORTS: usize = 3;
/// Nombre maximal de lignes affichées par rapport du journal Windows.
const MAX_APP_ERROR_LINES: usize = 14;

/// Session ETW active ; l'arrêt est automatique à la destruction.
pub struct TraceGuard {
    _trace: UserTrace,
}

/// Démarre la capture des démarrages/arrêts de processus.
///
/// Nécessite les droits administrateur (ou l'appartenance au groupe
/// « Performance Log Users »).
pub fn start_process_trace(sink: EventSink) -> Result<TraceGuard> {
    let callback = move |record: &EventRecord, locator: &SchemaLocator| {
        let id = record.event_id();
        if id != EVENT_PROCESS_START && id != EVENT_PROCESS_STOP {
            return;
        }
        let Ok(schema) = locator.event_schema(record) else {
            return;
        };
        let parser = Parser::create(record, &schema);
        let Ok(pid) = parser.try_parse::<u32>("ProcessID") else {
            sink.count_drop();
            return;
        };
        let image: String = parser.try_parse("ImageName").unwrap_or_default();
        // L'heure est celle de réception : ETW la fournit dans une horloge qui
        // dépend de la configuration de la session, inutile ici.
        let at = Utc::now();

        let kind = if id == EVENT_PROCESS_START {
            let parent_pid: u32 = parser.try_parse("ParentProcessID").unwrap_or(0);
            EventKind::Start(ProcessStart {
                pid,
                parent_pid,
                image,
                command_line: None,
            })
        } else {
            // Le code de sortie est normalement un entier non signé ; on tolère aussi
            // un entier signé. Sans code lisible, mieux vaut abandonner l'événement
            // (et le compter) que de faire passer un plantage pour une sortie normale.
            let Some(exit_code) = parser
                .try_parse::<u32>("ExitCode")
                .or_else(|_| parser.try_parse::<i32>("ExitCode").map(|code| code as u32))
                .ok()
            else {
                sink.count_drop();
                return;
            };
            EventKind::Stop(ProcessStop {
                pid,
                image,
                exit_code,
            })
        };
        sink.send(Event { at, kind });
    };

    let provider = Provider::by_guid(KERNEL_PROCESS_GUID)
        .any(KEYWORD_PROCESS)
        .add_callback(callback)
        .build();

    // Nom unique par instance : deux `procwatch` simultanés ne se gênent pas.
    let trace = UserTrace::new()
        .named(format!("procwatch-{}", std::process::id()))
        .enable(provider)
        .start_and_process()
        .map_err(|e| anyhow!("démarrage de la session ETW impossible : {e:?}"))?;
    Ok(TraceGuard { _trace: trace })
}

/// Indique si le processus tourne avec des droits d'administrateur élevés.
pub fn is_elevated() -> bool {
    // SAFETY : appels Win32 sur le jeton du processus courant ; le tampon
    // `elevation` a la taille annoncée et le handle est refermé dans tous les cas.
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION::default();
        let mut returned = 0u32;
        let queried = GetTokenInformation(
            token,
            TokenElevation,
            Some(std::ptr::addr_of_mut!(elevation).cast::<c_void>()),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
        .is_ok();
        let _ = CloseHandle(token);
        queried && elevation.TokenIsElevated != 0
    }
}

/// Canal vers la session, partagé avec le callback réseau (qui ne reçoit
/// qu'un pointeur brut). Une seule surveillance réseau par processus.
static NETWORK_SINK: OnceLock<EventSink> = OnceLock::new();

/// Surveillance réseau active ; l'inscription est annulée à la destruction.
pub struct NetworkGuard {
    handle: HANDLE,
}

// SAFETY : le handle n'est qu'un identifiant d'inscription, utilisable depuis n'importe quel thread.
unsafe impl Send for NetworkGuard {}

impl Drop for NetworkGuard {
    fn drop(&mut self) {
        // SAFETY : `handle` provient de `NotifyIpInterfaceChange` et n'est annulé qu'ici.
        unsafe {
            let _ = CancelMibChangeNotify2(self.handle);
        }
    }
}

unsafe extern "system" fn on_interface_change(
    _context: *const c_void,
    row: *const MIB_IPINTERFACE_ROW,
    kind: MIB_NOTIFICATION_TYPE,
) {
    // SAFETY : Windows garantit que `row`, quand il n'est pas nul, est valide pendant l'appel.
    let Some(row) = (unsafe { row.as_ref() }) else {
        return;
    };
    let kind = match kind.0 {
        0 => NetworkChangeKind::Modified,
        1 => NetworkChangeKind::Added,
        2 => NetworkChangeKind::Removed,
        _ => return,
    };
    if let Some(sink) = NETWORK_SINK.get() {
        sink.send(Event {
            at: Utc::now(),
            kind: EventKind::Network(NetworkChange {
                kind,
                interface_index: row.InterfaceIndex,
            }),
        });
    }
}

/// Démarre la détection des changements d'interfaces réseau (IPv4 et IPv6).
pub fn start_network_watch(sink: EventSink) -> Result<NetworkGuard> {
    NETWORK_SINK
        .set(sink)
        .map_err(|_| anyhow!("la surveillance réseau est déjà démarrée"))?;
    let mut handle = HANDLE::default();
    // SAFETY : le callback est une fonction `extern "system"` valide pour toute
    // la durée du processus ; `handle` reçoit l'identifiant d'inscription.
    let status = unsafe {
        NotifyIpInterfaceChange(
            AF_UNSPEC,
            Some(on_interface_change),
            None,
            false,
            &mut handle,
        )
    };
    status
        .ok()
        .context("inscription aux changements d'interface réseau impossible")?;
    Ok(NetworkGuard { handle })
}

/// Rapports d'erreur d'application (événements 1000/1001 du journal Windows)
/// des dernières secondes qui mentionnent `image`.
///
/// Les arguments de `wevtutil` sont entièrement construits à partir d'un nombre :
/// `image` ne sert qu'à filtrer la sortie, jamais à bâtir la requête.
pub fn recent_app_errors(image: &str, within_seconds: u64) -> Vec<String> {
    let query = format!(
        "*[System[(EventID=1000 or EventID=1001) and TimeCreated[timediff(@SystemTime) <= {}]]]",
        within_seconds.saturating_mul(1_000)
    );
    let output = Command::new("wevtutil")
        .args(["qe", "Application", "/c:30", "/rd:true", "/f:text"])
        .arg(format!("/q:{query}"))
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let needle = image.to_lowercase();
    text.split("Event[")
        .filter(|block| block.to_lowercase().contains(&needle))
        .take(MAX_APP_ERROR_REPORTS)
        .map(|block| {
            block
                .lines()
                .map(str::trim_end)
                .filter(|l| !l.trim().is_empty())
                .take(MAX_APP_ERROR_LINES)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect()
}
