//! Bus d'événements in-process.
//!
//! Tout ce qui se produit dans le moteur passe par ici. Trois consommateurs
//! indépendants s'y branchent — la persistance, le constructeur de snapshots,
//! le frontend — sans jamais se connaître.

use atelier_domain::{DomainEvent, LogLine};
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<DomainEvent>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    /// Publier n'échoue jamais du point de vue de l'appelant : s'il n'y a
    /// aucun abonné, l'événement est simplement perdu. Le moteur ne doit
    /// pas s'arrêter parce que l'UI est fermée.
    pub fn publish(&self, event: DomainEvent) {
        let _ = self.tx.send(event);
    }

    pub fn log(&self, line: LogLine) {
        self.publish(DomainEvent::Log(line));
    }

    /// Un abonné lent est déconnecté (`RecvError::Lagged`) plutôt que de
    /// faire grossir la file indéfiniment. Perdre des lignes d'affichage est
    /// préférable à une consommation mémoire non bornée.
    pub fn subscribe(&self) -> broadcast::Receiver<DomainEvent> {
        self.tx.subscribe()
    }

    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(2048)
    }
}
