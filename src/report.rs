//! Rapport affiché quand un processus cible se termine.

use chrono::Duration;

use crate::event::{Pid, Record};
use crate::exit_code::ExitKind;
use crate::render::{format_duration, record_line, Line, Tone};
use crate::verdict::Verdict;

/// Nombre maximal de processus masqués détaillés dans la synthèse.
const MAX_IGNORED_LISTED: usize = 5;

/// Retrait des lignes de détail pour les aligner sous l'étiquette `RAPPORT`.
const DETAIL_INDENT: &str = "         ";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub image: String,
    pub pid: Pid,
    /// Durée de vie, inconnue si le démarrage n'a pas été observé.
    pub lived: Option<Duration>,
    pub verdict: Verdict,
    /// Événements ayant précédé la fin, du plus ancien au plus récent.
    pub context: Vec<Record>,
    /// Processus masqués dans la fenêtre : (nom, occurrences), du plus fréquent au moins fréquent.
    pub ignored: Vec<(String, u64)>,
    pub window_seconds: i64,
}

impl Report {
    fn tone(&self) -> Tone {
        match self.verdict.kind {
            ExitKind::Crash => Tone::Crash,
            ExitKind::Error => Tone::Error,
            ExitKind::Clean | ExitKind::CtrlC => Tone::Clean,
        }
    }

    fn detail(&self, label: &str, text: String) -> Line {
        Line::new(
            Tone::Info,
            "",
            format!("{DETAIL_INDENT}{label:<13}: {text}"),
        )
    }

    /// Lignes du rapport, prêtes à être affichées.
    pub fn lines(&self) -> Vec<Line> {
        let mut out = Vec::new();
        out.push(
            Line::new(
                self.tone(),
                "RAPPORT",
                format!(
                    "===== Fin de la cible {} (pid {}) =====",
                    self.image, self.pid
                ),
            )
            .emphasized(),
        );
        if let Some(lived) = self.lived {
            out.push(self.detail("Durée de vie", format_duration(lived)));
        }
        out.push(
            Line::new(
                self.tone(),
                "",
                format!(
                    "{DETAIL_INDENT}{:<13}: {}",
                    "Verdict",
                    self.verdict.headline()
                ),
            )
            .emphasized(),
        );
        if let Some(command) = self
            .verdict
            .killer
            .as_ref()
            .and_then(|k| k.command_line.as_deref())
        {
            out.push(self.detail("Commande", command.to_owned()));
        }

        let mut context = format!(
            "{} événement(s) sur les {} s précédentes",
            self.context.len(),
            self.window_seconds
        );
        if !self.ignored.is_empty() {
            let total: u64 = self.ignored.iter().map(|(_, n)| n).sum();
            let listed: Vec<String> = self
                .ignored
                .iter()
                .take(MAX_IGNORED_LISTED)
                .map(|(name, n)| format!("{name} x{n}"))
                .collect();
            context.push_str(&format!(" ; {total} masqué(s) : {}", listed.join(", ")));
        }
        out.push(self.detail("Contexte", context));

        out.extend(
            self.context
                .iter()
                .map(|record| record_line(record).indented("| ")),
        );
        out.push(Line::new(self.tone(), "RAPPORT", "===== Fin du rapport =====").emphasized());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verdict::Suspect;

    fn report(killer: Option<Suspect>, ignored: Vec<(String, u64)>) -> Report {
        Report {
            image: "RobloxPlayerBeta.exe".into(),
            pid: 4432,
            lived: Some(Duration::seconds(123)),
            verdict: Verdict {
                kind: ExitKind::Error,
                exit_code: 1,
                killer,
            },
            context: Vec::new(),
            ignored,
            window_seconds: 30,
        }
    }

    #[test]
    fn report_mentions_duration_verdict_and_context() {
        let text: Vec<String> = report(None, vec![])
            .lines()
            .iter()
            .map(Line::plain)
            .collect();
        let all = text.join("\n");
        assert!(all.contains("Fin de la cible RobloxPlayerBeta.exe (pid 4432)"));
        assert!(all.contains("2 min 03 s"));
        assert!(all.contains("Verdict"));
        assert!(all.contains("0 événement(s) sur les 30 s précédentes"));
        assert!(all.contains("Fin du rapport"));
    }

    #[test]
    fn killer_command_is_listed() {
        let killer = Suspect {
            name: "taskkill.exe".into(),
            pid: 9,
            parent_name: Some("Fleasion.exe".into()),
            command_line: Some("taskkill /F /PID 4432".into()),
        };
        let all: String = report(Some(killer), vec![])
            .lines()
            .iter()
            .map(Line::plain)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(all.contains("Commande"));
        assert!(all.contains("taskkill /F /PID 4432"));
    }

    #[test]
    fn ignored_summary_is_capped_and_totalled() {
        let ignored: Vec<(String, u64)> = (0..8).map(|i| (format!("p{i}.exe"), 10 - i)).collect();
        let all: String = report(None, ignored)
            .lines()
            .iter()
            .map(Line::plain)
            .collect::<Vec<_>>()
            .join("\n");
        assert!(all.contains("masqué(s)"));
        assert!(all.contains("p0.exe x10"));
        assert!(!all.contains("p5.exe"));
    }
}
