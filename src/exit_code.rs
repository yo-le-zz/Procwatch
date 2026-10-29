//! Interprétation des codes de sortie Windows.
//!
//! Un code `0xC0000005` est un vrai plantage ; un code `1` est le plus souvent
//! un arrêt forcé (`taskkill /F`, `TerminateProcess`) ou une erreur générique :
//! la nuance est essentielle pour savoir si un processus a planté ou a été tué.

/// Catégorie d'une fin de processus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitKind {
    /// Code 0.
    Clean,
    /// Code non nul sans signature de plantage.
    Error,
    /// Exception non gérée (NTSTATUS d'erreur `0xC...`, `0xE...`, point d'arrêt).
    Crash,
    /// Interruption par Ctrl+C.
    CtrlC,
}

/// Plantages connus : (code, description).
const KNOWN: &[(u32, &str)] = &[
    (0xC000_0005, "violation d'accès mémoire"),
    (0xC000_0409, "dépassement de tampon / fail-fast"),
    (0xC000_00FD, "dépassement de pile"),
    (0xC000_0374, "corruption du tas"),
    (0xC000_0135, "DLL introuvable"),
    (0xC000_0142, "échec d'initialisation d'une DLL"),
    (0xC000_007B, "image invalide (32/64 bits)"),
    (0xC000_0094, "division entière par zéro"),
    (0xC000_001D, "instruction illégale"),
    (0xC000_0096, "instruction privilégiée"),
    (0xC000_0017, "mémoire insuffisante"),
    (0xC000_013A, "interrompu par Ctrl+C"),
    (0xE06D_7363, "exception C++ non interceptée"),
    (0x8000_0003, "point d'arrêt"),
    (0x4001_0004, "terminé par un débogueur"),
    (0x4001_0005, "Ctrl+C reçu par un débogueur"),
];

fn lookup(code: u32) -> Option<&'static str> {
    KNOWN.iter().find(|(c, _)| *c == code).map(|(_, d)| *d)
}

pub fn classify(code: u32) -> ExitKind {
    match code {
        0 => ExitKind::Clean,
        0xC000_013A | 0x4001_0005 => ExitKind::CtrlC,
        0x8000_0003 => ExitKind::Crash,
        c if matches!(c >> 28, 0xC | 0xE) => ExitKind::Crash,
        _ => ExitKind::Error,
    }
}

/// Texte lisible : `0xC0000005 (violation d'accès mémoire)`.
pub fn describe(code: u32) -> String {
    match (code, lookup(code)) {
        (0, _) => "0x0 (sortie normale)".to_owned(),
        (1, _) => {
            "0x1 (erreur générique ou arrêt forcé : taskkill /F, TerminateProcess)".to_owned()
        }
        (c, Some(text)) => format!("{c:#X} ({text})"),
        (c, None) if classify(c) == ExitKind::Crash => format!("{c:#X} (exception système)"),
        (c, None) => format!("{c:#X} (code d'erreur applicatif)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_is_clean() {
        assert_eq!(classify(0), ExitKind::Clean);
    }

    #[test]
    fn access_violation_is_a_crash() {
        assert_eq!(classify(0xC000_0005), ExitKind::Crash);
        assert!(describe(0xC000_0005).contains("violation d'accès"));
    }

    #[test]
    fn unknown_ntstatus_error_is_still_a_crash() {
        assert_eq!(classify(0xC123_4567), ExitKind::Crash);
        assert!(describe(0xC123_4567).contains("exception système"));
    }

    #[test]
    fn cpp_exception_and_breakpoint_are_crashes() {
        assert_eq!(classify(0xE06D_7363), ExitKind::Crash);
        assert_eq!(classify(0x8000_0003), ExitKind::Crash);
    }

    #[test]
    fn ctrl_c_is_not_a_crash() {
        assert_eq!(classify(0xC000_013A), ExitKind::CtrlC);
    }

    #[test]
    fn small_codes_are_plain_errors() {
        assert_eq!(classify(1), ExitKind::Error);
        assert_eq!(classify(0xFFFF_FFFF), ExitKind::Error);
        assert!(describe(1).contains("taskkill"));
    }
}
