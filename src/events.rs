use crate::models::StarSyncEvent;
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct EventBus {
    sender: broadcast::Sender<StarSyncEvent>,
}

impl EventBus {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(256);
        Self { sender }
    }

    pub fn emit(&self, event: StarSyncEvent) {
        let _ = self.sender.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<StarSyncEvent> {
        self.sender.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}
