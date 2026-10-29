//! # procwatch
//!
//! Surveillance en temps réel des processus Windows (ETW) pour comprendre
//! *pourquoi* un programme s'est arrêté : plantage, arrêt forcé par un autre
//! outil, ou fermeture normale.
//!
//! La logique (filtres, tampon d'historique, verdict, rapport) est
//! indépendante de la plateforme et testable ; seule la collecte des événements
//! vit dans [`platform`] et ne fonctionne que sous Windows.

pub mod buffer;
pub mod config;
pub mod event;
pub mod exit_code;
pub mod inspector;
pub mod matcher;
pub mod platform;
pub mod render;
pub mod report;
pub mod session;
pub mod table;
pub mod verdict;
