use crate::models::{
    EventEnvelope, EventSubscriptionCreate, EventSubscriptionPatch, EventSubscriptionView,
    StarSyncEvent, WebhookDeliveryState,
};
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::sync::broadcast;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

#[derive(Clone)]
pub struct EventBus {
    inner: Arc<EventBusInner>,
}

struct EventBusInner {
    sender: broadcast::Sender<EventEnvelope>,
    events_file: PathBuf,
    subscriptions_file: PathBuf,
    subscriptions: Mutex<Vec<EventSubscriptionRecord>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct EventSubscriptionRecord {
    id: String,
    url: String,
    events: Vec<String>,
    enabled: bool,
    secret: Option<String>,
    created_at: chrono::DateTime<Utc>,
    updated_at: chrono::DateTime<Utc>,
    failure_count: u64,
    last_delivery: Option<WebhookDeliveryState>,
}

impl EventBus {
    pub fn new(events_file: PathBuf, subscriptions_file: PathBuf) -> Self {
        let (sender, _) = broadcast::channel(256);
        let subscriptions = read_subscriptions(&subscriptions_file).unwrap_or_default();
        Self {
            inner: Arc::new(EventBusInner {
                sender,
                events_file,
                subscriptions_file,
                subscriptions: Mutex::new(subscriptions),
            }),
        }
    }

    pub fn emit(&self, event: StarSyncEvent) {
        let envelope = EventEnvelope {
            id: Uuid::new_v4().to_string(),
            name: event_name(&event).to_string(),
            emitted_at: Utc::now(),
            event,
        };
        let _ = append_event(&self.inner.events_file, &envelope);
        let _ = self.inner.sender.send(envelope.clone());
        self.dispatch_webhooks(envelope);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EventEnvelope> {
        self.inner.sender.subscribe()
    }

    pub fn recent(&self, limit: usize) -> Result<Vec<EventEnvelope>> {
        let limit = limit.clamp(1, 500);
        if !self.inner.events_file.exists() {
            return Ok(Vec::new());
        }
        let raw = fs::read_to_string(&self.inner.events_file).with_context(|| {
            format!(
                "failed to read event log {}",
                self.inner.events_file.display()
            )
        })?;
        let mut events = raw
            .lines()
            .filter(|line| !line.trim().is_empty())
            .filter_map(|line| serde_json::from_str::<EventEnvelope>(line).ok())
            .collect::<Vec<_>>();
        if events.len() > limit {
            events = events.split_off(events.len() - limit);
        }
        Ok(events)
    }

    pub fn list_subscriptions(&self) -> Vec<EventSubscriptionView> {
        self.inner
            .subscriptions
            .lock()
            .unwrap()
            .iter()
            .map(EventSubscriptionRecord::view)
            .collect()
    }

    pub fn create_subscription(
        &self,
        create: EventSubscriptionCreate,
    ) -> Result<EventSubscriptionView> {
        validate_url(&create.url)?;
        let now = Utc::now();
        let record = EventSubscriptionRecord {
            id: Uuid::new_v4().to_string(),
            url: create.url,
            events: normalize_events(create.events),
            enabled: create.enabled,
            secret: create.secret.filter(|secret| !secret.is_empty()),
            created_at: now,
            updated_at: now,
            failure_count: 0,
            last_delivery: None,
        };
        let view = record.view();
        let mut subscriptions = self.inner.subscriptions.lock().unwrap();
        subscriptions.push(record);
        write_subscriptions(&self.inner.subscriptions_file, &subscriptions)?;
        Ok(view)
    }

    pub fn patch_subscription(
        &self,
        id: &str,
        patch: EventSubscriptionPatch,
    ) -> Result<EventSubscriptionView> {
        let mut subscriptions = self.inner.subscriptions.lock().unwrap();
        let record = subscriptions
            .iter_mut()
            .find(|record| record.id == id)
            .ok_or_else(|| anyhow!("event subscription not found: {id}"))?;
        if let Some(url) = patch.url {
            validate_url(&url)?;
            record.url = url;
        }
        if let Some(events) = patch.events {
            record.events = normalize_events(events);
        }
        if let Some(enabled) = patch.enabled {
            record.enabled = enabled;
        }
        if let Some(secret) = patch.secret {
            record.secret = secret.filter(|value| !value.is_empty());
        }
        record.updated_at = Utc::now();
        let view = record.view();
        write_subscriptions(&self.inner.subscriptions_file, &subscriptions)?;
        Ok(view)
    }

    pub fn delete_subscription(&self, id: &str) -> Result<EventSubscriptionView> {
        let mut subscriptions = self.inner.subscriptions.lock().unwrap();
        let index = subscriptions
            .iter()
            .position(|record| record.id == id)
            .ok_or_else(|| anyhow!("event subscription not found: {id}"))?;
        let record = subscriptions.remove(index);
        write_subscriptions(&self.inner.subscriptions_file, &subscriptions)?;
        Ok(record.view())
    }

    fn dispatch_webhooks(&self, envelope: EventEnvelope) {
        let subscriptions = self
            .inner
            .subscriptions
            .lock()
            .unwrap()
            .iter()
            .filter(|record| record.matches(&envelope.name))
            .cloned()
            .collect::<Vec<_>>();
        for subscription in subscriptions {
            let bus = self.clone();
            let envelope = envelope.clone();
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    let delivery = deliver_webhook(&subscription, &envelope).await;
                    bus.record_delivery(&subscription.id, delivery);
                });
            }
        }
    }

    fn record_delivery(&self, id: &str, delivery: WebhookDeliveryState) {
        let mut subscriptions = self.inner.subscriptions.lock().unwrap();
        let Some(record) = subscriptions.iter_mut().find(|record| record.id == id) else {
            return;
        };
        if delivery.success {
            record.failure_count = 0;
        } else {
            record.failure_count = record.failure_count.saturating_add(1);
        }
        record.last_delivery = Some(delivery);
        record.updated_at = Utc::now();
        let _ = write_subscriptions(&self.inner.subscriptions_file, &subscriptions);
    }
}

impl EventSubscriptionRecord {
    fn view(&self) -> EventSubscriptionView {
        EventSubscriptionView {
            id: self.id.clone(),
            url: self.url.clone(),
            events: self.events.clone(),
            enabled: self.enabled,
            created_at: self.created_at,
            updated_at: self.updated_at,
            failure_count: self.failure_count,
            last_delivery: self.last_delivery.clone(),
        }
    }

    fn matches(&self, event_name: &str) -> bool {
        self.enabled
            && (self.events.iter().any(|event| event == "*")
                || self.events.iter().any(|event| event == event_name))
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new(
            PathBuf::from("events.jsonl"),
            PathBuf::from("subscriptions.json"),
        )
    }
}

pub fn event_name(event: &StarSyncEvent) -> &'static str {
    match event {
        StarSyncEvent::TaskStarted { .. } => "task.started",
        StarSyncEvent::TaskCompleted { .. } => "task.completed",
        StarSyncEvent::TaskFailed { .. } => "task.failed",
        StarSyncEvent::SyncStarted { .. } => "sync.started",
        StarSyncEvent::RemoteAdded { .. } => "repo.added",
        StarSyncEvent::RemoteRemoved { .. } => "repo.removed",
        StarSyncEvent::RemoteUpdated { .. } => "repo.updated",
        StarSyncEvent::MetaChanged { .. } => "meta.changed",
        StarSyncEvent::ReadmeEnriched { .. } => "readme.enriched",
        StarSyncEvent::SyncCompleted { .. } => "sync.completed",
        StarSyncEvent::StorageChanged { .. } => "storage.changed",
        StarSyncEvent::Error { .. } => "error",
    }
}

fn normalize_events(events: Vec<String>) -> Vec<String> {
    let mut events = events
        .into_iter()
        .map(|event| event.trim().to_string())
        .filter(|event| !event.is_empty())
        .collect::<Vec<_>>();
    if events.is_empty() {
        events.push("*".to_string());
    }
    events.sort();
    events.dedup();
    events
}

fn validate_url(url: &str) -> Result<()> {
    let parsed = reqwest::Url::parse(url).with_context(|| format!("invalid webhook URL: {url}"))?;
    match parsed.scheme() {
        "http" | "https" => Ok(()),
        other => Err(anyhow!("unsupported webhook URL scheme: {other}")),
    }
}

fn append_event(path: &Path, envelope: &EventEnvelope) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let mut line = serde_json::to_string(envelope)?;
    line.push('\n');
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open event log {}", path.display()))?;
    file.write_all(line.as_bytes())?;
    Ok(())
}

fn read_subscriptions(path: &Path) -> Result<Vec<EventSubscriptionRecord>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(path)
        .with_context(|| format!("failed to read subscriptions {}", path.display()))?;
    Ok(serde_json::from_str(&raw)?)
}

fn write_subscriptions(path: &Path, records: &[EventSubscriptionRecord]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(path, serde_json::to_string_pretty(records)?)
        .with_context(|| format!("failed to write subscriptions {}", path.display()))?;
    Ok(())
}

async fn deliver_webhook(
    subscription: &EventSubscriptionRecord,
    envelope: &EventEnvelope,
) -> WebhookDeliveryState {
    let delivered_at = Utc::now();
    let payload = match serde_json::to_vec(envelope) {
        Ok(payload) => payload,
        Err(error) => {
            return WebhookDeliveryState {
                delivered_at,
                success: false,
                status: None,
                error: Some(error.to_string()),
            }
        }
    };
    let mut request = reqwest::Client::new()
        .post(&subscription.url)
        .header("content-type", "application/json")
        .header("x-starsync-event", &envelope.name)
        .header("x-starsync-delivery", &envelope.id)
        .body(payload.clone());
    if let Some(secret) = &subscription.secret {
        request = request.header("x-starsync-signature-256", signature(secret, &payload));
    }
    match request.send().await {
        Ok(response) => {
            let status = response.status().as_u16();
            WebhookDeliveryState {
                delivered_at,
                success: response.status().is_success(),
                status: Some(status),
                error: None,
            }
        }
        Err(error) => WebhookDeliveryState {
            delivered_at,
            success: false,
            status: None,
            error: Some(error.to_string()),
        },
    }
}

fn signature(secret: &str, payload: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key size");
    mac.update(payload);
    format!("sha256={}", hex::encode(mac.finalize().into_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_recent_events_and_manages_subscriptions() {
        let dir = tempfile::tempdir().unwrap();
        let bus = EventBus::new(
            dir.path().join("events.jsonl"),
            dir.path().join("subscriptions.json"),
        );
        bus.emit(StarSyncEvent::RemoteAdded {
            repo: "alice/demo".to_string(),
        });

        let recent = bus.recent(10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].name, "repo.added");

        let sub = bus
            .create_subscription(EventSubscriptionCreate {
                url: "https://example.com/hook".to_string(),
                events: vec!["repo.added".to_string()],
                enabled: true,
                secret: Some("secret".to_string()),
            })
            .unwrap();
        assert_eq!(bus.list_subscriptions().len(), 1);

        let updated = bus
            .patch_subscription(
                &sub.id,
                EventSubscriptionPatch {
                    enabled: Some(false),
                    ..EventSubscriptionPatch::default()
                },
            )
            .unwrap();
        assert!(!updated.enabled);

        bus.delete_subscription(&sub.id).unwrap();
        assert!(bus.list_subscriptions().is_empty());
    }
}
