//! Rejoue, sans ETW ni droits administrateur, un scénario réel : un outil lance
//! `taskkill /F` sur Roblox, qui disparaît avec le code 1.
//!
//! Utile pour découvrir le rendu du rapport sur n'importe quel système :
//! `cargo run --example replay`

use std::io::{self, IsTerminal};

use chrono::{Duration, Utc};
use procwatch::config::Config;
use procwatch::event::{Event, EventKind, ProcessStart, ProcessStop};
use procwatch::inspector::ProcessInfo;
use procwatch::session::{Session, SessionOptions};

fn start(
    at: chrono::DateTime<Utc>,
    pid: u32,
    parent: u32,
    image: &str,
    cmd: Option<&str>,
) -> Event {
    Event {
        at,
        kind: EventKind::Start(ProcessStart {
            pid,
            parent_pid: parent,
            image: image.to_owned(),
            command_line: cmd.map(str::to_owned),
        }),
    }
}

fn main() -> io::Result<()> {
    let config = Config {
        targets: vec!["RobloxPlayerBeta".to_owned()],
        ..Config::default()
    };
    let color = io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
    let mut session = Session::new(
        SessionOptions::from_config(&config, color),
        Box::new(io::stdout()),
        None,
        Box::new(|_| None),
    );

    let t0 = Utc::now() - Duration::seconds(130);
    let at = |secs: f64| t0 + Duration::milliseconds((secs * 1000.0) as i64);

    session.seed(&[ProcessInfo {
        pid: 100,
        parent_pid: None,
        name: "explorer.exe".to_owned(),
    }])?;
    session.handle(start(at(0.0), 200, 100, "Fleasion.exe", None))?;
    session.handle(start(at(2.0), 4432, 200, "RobloxPlayerBeta.exe", None))?;
    for i in 0..40u32 {
        session.handle(start(
            at(100.0 + f64::from(i) * 0.01),
            9_000 + i,
            1,
            "NetCfgNotifyObjectHost.exe",
            None,
        ))?;
    }
    session.handle(start(
        at(125.0),
        20_112,
        200,
        "taskkill.exe",
        Some("taskkill /F /PID 4432"),
    ))?;
    session.handle(Event {
        at: at(125.6),
        kind: EventKind::Stop(ProcessStop {
            pid: 4432,
            image: "RobloxPlayerBeta.exe".to_owned(),
            exit_code: 1,
        }),
    })?;
    Ok(())
}
