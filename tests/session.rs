//! Scénarios de bout en bout de la session, avec des événements synthétiques.

use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, TimeZone, Utc};
use procwatch::config::Config;
use procwatch::event::{
    Event, EventKind, NetworkChange, NetworkChangeKind, ProcessStart, ProcessStop,
};
use procwatch::exit_code::ExitKind;
use procwatch::inspector::ProcessInfo;
use procwatch::session::{CommandLineLookup, Session, SessionOptions};

/// Tampon partagé : permet de relire ce que la session a écrit.
#[derive(Clone, Default)]
struct SharedBuf(Arc<Mutex<Vec<u8>>>);

impl SharedBuf {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
    }
}

impl Write for SharedBuf {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Écrivain qui échoue toujours (disque plein, fichier supprimé…).
struct FailingWriter;

impl Write for FailingWriter {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("disque plein"))
    }
    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::other("disque plein"))
    }
}

fn at(millis: i64) -> DateTime<Utc> {
    Utc.timestamp_millis_opt(1_700_000_000_000 + millis)
        .unwrap()
}

fn start(ms: i64, pid: u32, parent: u32, image: &str, cmd: Option<&str>) -> Event {
    Event {
        at: at(ms),
        kind: EventKind::Start(ProcessStart {
            pid,
            parent_pid: parent,
            image: image.to_owned(),
            command_line: cmd.map(str::to_owned),
        }),
    }
}

fn stop(ms: i64, pid: u32, image: &str, exit_code: u32) -> Event {
    Event {
        at: at(ms),
        kind: EventKind::Stop(ProcessStop {
            pid,
            image: image.to_owned(),
            exit_code,
        }),
    }
}

fn network(ms: i64, kind: NetworkChangeKind, index: u32) -> Event {
    Event {
        at: at(ms),
        kind: EventKind::Network(NetworkChange {
            kind,
            interface_index: index,
        }),
    }
}

fn no_lookup() -> CommandLineLookup {
    Box::new(|_| None)
}

fn config(targets: &[&str]) -> Config {
    Config {
        targets: targets.iter().map(|t| (*t).to_owned()).collect(),
        ..Config::default()
    }
}

fn session(config: &Config, color: bool, term: &SharedBuf, lookup: CommandLineLookup) -> Session {
    Session::new(
        SessionOptions::from_config(config, color),
        Box::new(term.clone()),
        None,
        lookup,
    )
}

fn process(pid: u32, name: &str) -> ProcessInfo {
    ProcessInfo {
        pid,
        parent_pid: None,
        name: name.to_owned(),
    }
}

#[test]
fn forced_kill_by_a_tool_is_attributed_to_it() {
    let term = SharedBuf::default();
    let mut s = session(&config(&["RobloxPlayerBeta"]), false, &term, no_lookup());
    s.seed(&[process(100, "explorer.exe")]).unwrap();

    s.handle(start(0, 200, 100, "Fleasion.exe", None)).unwrap();
    s.handle(start(
        1_000,
        4432,
        200,
        r"\Device\HarddiskVolume3\Roblox\RobloxPlayerBeta.exe",
        None,
    ))
    .unwrap();
    // Rafale de bruit : doit être masquée mais comptée.
    for i in 0..50 {
        s.handle(start(
            2_000 + i,
            9_000 + i as u32,
            1,
            "NetCfgNotifyObjectHost.exe",
            None,
        ))
        .unwrap();
    }
    s.handle(start(
        120_000,
        20112,
        200,
        "taskkill.exe",
        Some("taskkill /F /PID 4432"),
    ))
    .unwrap();
    let report = s
        .handle(stop(120_500, 4432, "RobloxPlayerBeta.exe", 1))
        .unwrap()
        .expect("rapport attendu");

    assert_eq!(report.verdict.kind, ExitKind::Error);
    let killer = report.verdict.killer.as_ref().expect("coupable attendu");
    assert_eq!(killer.name, "taskkill.exe");
    assert_eq!(killer.parent_name.as_deref(), Some("Fleasion.exe"));
    assert_eq!(report.lived.unwrap().num_seconds(), 119);

    let out = term.text();
    assert!(out.contains("CIBLE>"));
    assert!(out.contains("CIBLE<"));
    assert!(out.contains("ARRÊT FORCÉ par taskkill.exe"));
    assert!(out.contains("taskkill /F /PID 4432"));
    assert!(
        !out.contains("NetCfgNotifyObjectHost.exe (pid"),
        "le bruit doit rester masqué"
    );
    assert_eq!(s.stats().reports, 1);
}

#[test]
fn noise_older_than_the_window_is_not_counted_in_the_report() {
    let term = SharedBuf::default();
    let mut s = session(&config(&["app"]), false, &term, no_lookup());
    s.handle(start(0, 1, 0, "app.exe", None)).unwrap();
    for i in 0..5 {
        s.handle(start(i, 50 + i as u32, 1, "conhost.exe", None))
            .unwrap();
    }
    for i in 0..3 {
        s.handle(start(100_000 + i, 60 + i as u32, 1, "conhost.exe", None))
            .unwrap();
    }
    let report = s.handle(stop(100_100, 1, "app.exe", 0)).unwrap().unwrap();
    assert_eq!(report.ignored, vec![("conhost.exe".to_owned(), 3)]);
}

#[test]
fn command_line_is_read_through_the_lookup_when_missing() {
    let term = SharedBuf::default();
    let lookup: CommandLineLookup =
        Box::new(|pid| (pid == 7).then(|| "taskkill /F /PID 4432".to_owned()));
    let mut s = session(&config(&["app"]), false, &term, lookup);
    s.handle(start(0, 4432, 1, "app.exe", None)).unwrap();
    s.handle(start(500, 7, 1, "taskkill.exe", None)).unwrap();
    let report = s.handle(stop(900, 4432, "app.exe", 1)).unwrap().unwrap();
    assert!(report.verdict.killer.is_some());
}

#[test]
fn crash_is_reported_as_crash() {
    let term = SharedBuf::default();
    let mut s = session(&config(&["app"]), false, &term, no_lookup());
    s.handle(start(0, 1, 0, "app.exe", None)).unwrap();
    let report = s
        .handle(stop(5_000, 1, "app.exe", 0xC000_0005))
        .unwrap()
        .unwrap();
    assert_eq!(report.verdict.kind, ExitKind::Crash);
    assert!(term.text().contains("PLANTAGE"));
    assert!(term.text().contains("violation d'accès"));
}

#[test]
fn clean_exit_is_reported_as_such() {
    let term = SharedBuf::default();
    let mut s = session(&config(&["app"]), false, &term, no_lookup());
    s.handle(start(0, 1, 0, "app.exe", None)).unwrap();
    let report = s.handle(stop(1_000, 1, "app.exe", 0)).unwrap().unwrap();
    assert_eq!(report.verdict.kind, ExitKind::Clean);
    assert!(term.text().contains("Sortie normale"));
}

#[test]
fn non_target_stop_gives_no_report() {
    let term = SharedBuf::default();
    let mut s = session(&config(&["app"]), false, &term, no_lookup());
    s.handle(start(0, 1, 0, "other.exe", None)).unwrap();
    assert!(s.handle(stop(10, 1, "other.exe", 0)).unwrap().is_none());
    assert_eq!(s.stats().reports, 0);
}

#[test]
fn target_started_before_the_session_is_tracked_without_lifetime() {
    let term = SharedBuf::default();
    let mut s = session(&config(&["app"]), false, &term, no_lookup());
    s.seed(&[process(42, "app.exe")]).unwrap();
    assert!(term
        .text()
        .contains("cible déjà en cours : app.exe (pid 42)"));
    // L'image n'est pas fournie à l'arrêt : le nom vient de la table.
    let report = s.handle(stop(500, 42, "", 0)).unwrap().unwrap();
    assert_eq!(report.image, "app.exe");
    assert!(report.lived.is_none());
}

#[test]
fn target_is_recognised_by_name_even_if_its_start_was_missed() {
    let term = SharedBuf::default();
    let mut s = session(&config(&["app"]), false, &term, no_lookup());
    assert!(s.handle(stop(10, 99, "app.exe", 0)).unwrap().is_some());
}

#[test]
fn ignored_processes_are_shown_when_requested() {
    let term = SharedBuf::default();
    let cfg = Config {
        show_ignored: true,
        ..config(&["app"])
    };
    let mut s = session(&cfg, false, &term, no_lookup());
    s.handle(start(0, 5, 1, "conhost.exe", None)).unwrap();
    assert!(term.text().contains("conhost.exe (pid 5)"));
    assert_eq!(s.stats().ignored, 0);
}

#[test]
fn a_target_is_never_hidden_even_if_it_matches_the_ignore_list() {
    let term = SharedBuf::default();
    let cfg = Config {
        ignore: vec!["app".into()],
        ..config(&["app"])
    };
    let mut s = session(&cfg, false, &term, no_lookup());
    s.handle(start(0, 1, 0, "app.exe", None)).unwrap();
    assert!(term.text().contains("CIBLE>"));
}

#[test]
fn parent_name_survives_the_parent_exit() {
    let term = SharedBuf::default();
    let cfg = Config {
        ignore: vec![],
        ..config(&["child"])
    };
    let mut s = session(&cfg, false, &term, no_lookup());
    s.handle(start(0, 10, 1, "launcher.exe", None)).unwrap();
    s.handle(stop(100, 10, "launcher.exe", 0)).unwrap();
    s.handle(start(200, 11, 10, "child.exe", None)).unwrap();
    assert!(term.text().contains("parent=launcher.exe(10)"));
}

#[test]
fn bursts_of_identical_network_events_are_merged() {
    let term = SharedBuf::default();
    let mut s = session(&config(&["app"]), false, &term, no_lookup());
    s.handle(network(0, NetworkChangeKind::Modified, 7))
        .unwrap();
    s.handle(network(300, NetworkChangeKind::Modified, 7))
        .unwrap();
    s.handle(network(600, NetworkChangeKind::Modified, 7))
        .unwrap();
    s.handle(network(2_000, NetworkChangeKind::Modified, 7))
        .unwrap();
    s.handle(network(2_100, NetworkChangeKind::Removed, 7))
        .unwrap();
    let lines = term.text().lines().filter(|l| l.contains("NET")).count();
    assert_eq!(lines, 3);
}

#[test]
fn network_events_can_be_disabled() {
    let term = SharedBuf::default();
    let cfg = Config {
        show_network: false,
        ..config(&["app"])
    };
    let mut s = session(&cfg, false, &term, no_lookup());
    s.handle(network(0, NetworkChangeKind::Added, 3)).unwrap();
    assert!(term.text().is_empty());
}

#[test]
fn network_events_appear_in_the_report_context() {
    let term = SharedBuf::default();
    let mut s = session(&config(&["app"]), false, &term, no_lookup());
    s.handle(start(0, 1, 0, "app.exe", None)).unwrap();
    s.handle(network(1_000, NetworkChangeKind::Removed, 4))
        .unwrap();
    let report = s.handle(stop(2_000, 1, "app.exe", 1)).unwrap().unwrap();
    assert!(report
        .context
        .iter()
        .any(|r| matches!(r.event.kind, EventKind::Network(_))));
}

#[test]
fn colour_goes_to_the_terminal_only_and_the_log_stays_plain() {
    let term = SharedBuf::default();
    let log = SharedBuf::default();
    let mut s = Session::new(
        SessionOptions::from_config(&config(&["app"]), true),
        Box::new(term.clone()),
        Some(Box::new(log.clone())),
        no_lookup(),
    );
    s.handle(start(0, 1, 0, "app.exe", None)).unwrap();
    assert!(term.text().contains('\u{1b}'));
    assert!(!log.text().contains('\u{1b}'));
    assert!(log.text().contains("CIBLE>"));
}

#[test]
fn a_failing_log_file_does_not_stop_the_watch() {
    let term = SharedBuf::default();
    let mut s = Session::new(
        SessionOptions::from_config(&config(&["app"]), false),
        Box::new(term.clone()),
        Some(Box::new(FailingWriter)),
        no_lookup(),
    );
    s.handle(start(0, 1, 0, "app.exe", None)).unwrap();
    s.handle(stop(10, 1, "app.exe", 0)).unwrap();
    let out = term.text();
    assert_eq!(
        out.matches("journal fichier désactivé").count(),
        1,
        "un seul avertissement"
    );
    assert!(out.contains("CIBLE<"), "la surveillance continue");
}

#[test]
fn a_failing_terminal_is_reported_to_the_caller() {
    let mut s = Session::new(
        SessionOptions::from_config(&config(&["app"]), false),
        Box::new(FailingWriter),
        None,
        no_lookup(),
    );
    assert!(s.handle(start(0, 1, 0, "app.exe", None)).is_err());
}

#[test]
fn context_lists_events_in_chronological_order_and_excludes_the_stop() {
    let term = SharedBuf::default();
    let mut s = session(&config(&["app"]), false, &term, no_lookup());
    s.handle(start(0, 1, 0, "app.exe", None)).unwrap();
    s.handle(start(100, 2, 1, "a.exe", None)).unwrap();
    s.handle(start(200, 3, 1, "b.exe", None)).unwrap();
    let report = s.handle(stop(300, 1, "app.exe", 0)).unwrap().unwrap();
    let names: Vec<&str> = report
        .context
        .iter()
        .filter_map(|r| match &r.event.kind {
            EventKind::Start(p) => Some(p.image.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(names, vec!["app.exe", "a.exe", "b.exe"]);
    assert!(!report
        .context
        .iter()
        .any(|r| matches!(r.event.kind, EventKind::Stop(_))));
}
