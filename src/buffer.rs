//! Tampon circulaire borné à la fois en durée et en nombre d'éléments.
//!
//! Il conserve les dernières secondes d'activité pour pouvoir, au moment où le
//! processus surveillé se termine, afficher ce qui s'est passé juste avant.

use std::collections::VecDeque;

use chrono::{DateTime, Duration, Utc};

#[derive(Debug)]
pub struct TimedBuffer<T> {
    items: VecDeque<(DateTime<Utc>, T)>,
    window: Duration,
    capacity: usize,
}

impl<T> TimedBuffer<T> {
    /// `capacity` est borné à au moins 1 pour éviter un tampon inutilisable.
    pub fn new(window: Duration, capacity: usize) -> Self {
        Self {
            items: VecDeque::new(),
            window,
            capacity: capacity.max(1),
        }
    }

    /// Ajoute un élément puis purge ce qui sort de la fenêtre ou de la capacité.
    ///
    /// La purge se fait par rapport à l'heure de l'élément ajouté et non à
    /// l'horloge système : le comportement reste déterministe (et testable).
    pub fn push(&mut self, at: DateTime<Utc>, item: T) {
        self.items.push_back((at, item));
        while self.items.len() > self.capacity {
            self.items.pop_front();
        }
        let oldest_allowed = at - self.window;
        while self.items.front().is_some_and(|(t, _)| *t < oldest_allowed) {
            self.items.pop_front();
        }
    }

    /// Éléments dont l'heure est dans `[until - within, until]`, du plus ancien au plus récent.
    pub fn recent(&self, until: DateTime<Utc>, within: Duration) -> impl Iterator<Item = &T> {
        let from = until - within;
        self.items
            .iter()
            .filter(move |(t, _)| *t >= from && *t <= until)
            .map(|(_, item)| item)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t(sec: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000 + sec, 0).unwrap()
    }

    #[test]
    fn drops_items_older_than_window() {
        let mut b = TimedBuffer::new(Duration::seconds(10), 100);
        b.push(t(0), "a");
        b.push(t(5), "b");
        b.push(t(20), "c");
        assert_eq!(b.len(), 1);
        assert_eq!(
            b.recent(t(20), Duration::seconds(10)).collect::<Vec<_>>(),
            vec![&"c"]
        );
    }

    #[test]
    fn respects_capacity() {
        let mut b = TimedBuffer::new(Duration::seconds(1000), 3);
        for i in 0..10 {
            b.push(t(i), i);
        }
        assert_eq!(b.len(), 3);
        assert_eq!(
            b.recent(t(9), Duration::seconds(1000))
                .copied()
                .collect::<Vec<_>>(),
            vec![7, 8, 9]
        );
    }

    #[test]
    fn zero_capacity_is_clamped() {
        let mut b = TimedBuffer::new(Duration::seconds(10), 0);
        b.push(t(0), 1);
        assert_eq!(b.len(), 1);
    }

    #[test]
    fn recent_filters_by_requested_range() {
        let mut b = TimedBuffer::new(Duration::seconds(100), 100);
        b.push(t(0), "old");
        b.push(t(50), "mid");
        b.push(t(60), "late");
        let got: Vec<_> = b.recent(t(60), Duration::seconds(15)).copied().collect();
        assert_eq!(got, vec!["mid", "late"]);
        assert!(b.recent(t(60), Duration::seconds(0)).copied().eq(["late"]));
    }
}
