use crate::protocol::{now_ms, Envelope};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use tokio::sync::RwLock;
use uuid::Uuid;

const MAX_TRANSIENT_SEEN: usize = 65_536;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredMessage {
    pub envelope: Envelope,
    pub ingress_peer: Option<String>,
    pub acked_peers: BTreeSet<String>,
    pub last_attempt_ms_by_peer: BTreeMap<String, u64>,
    pub stored_at_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InboxEntry {
    pub envelope: Envelope,
    pub received_at_ms: u64,
}

#[derive(Default)]
struct StorageData {
    messages: HashMap<Uuid, StoredMessage>,
    inbox: HashMap<Uuid, InboxEntry>,
    durable_seen: HashSet<Uuid>,
    transient_seen: HashSet<Uuid>,
    transient_seen_order: VecDeque<Uuid>,
}

pub struct Storage {
    queue_dir: PathBuf,
    inbox_dir: PathBuf,
    seen_dir: PathBuf,
    data: RwLock<StorageData>,
}

impl Storage {
    pub async fn open(root: PathBuf) -> Result<Self> {
        let queue_dir = root.join("queue");
        let inbox_dir = root.join("inbox");
        let seen_dir = root.join("seen");
        tokio::fs::create_dir_all(&queue_dir)
            .await
            .with_context(|| format!("failed to create {}", queue_dir.display()))?;
        tokio::fs::create_dir_all(&inbox_dir)
            .await
            .with_context(|| format!("failed to create {}", inbox_dir.display()))?;
        tokio::fs::create_dir_all(&seen_dir)
            .await
            .with_context(|| format!("failed to create {}", seen_dir.display()))?;

        let messages = load_bin_directory::<StoredMessage>(&queue_dir).await?;
        let inbox = load_bin_directory::<InboxEntry>(&inbox_dir).await?;
        let seen = load_seen_directory(&seen_dir).await?;

        let mut data = StorageData::default();
        for message in messages {
            data.durable_seen.insert(message.envelope.message_id);
            data.messages.insert(message.envelope.message_id, message);
        }
        for item in inbox {
            data.durable_seen.insert(item.envelope.message_id);
            data.inbox.insert(item.envelope.message_id, item);
        }
        for id in seen {
            data.durable_seen.insert(id);
        }

        Ok(Self {
            queue_dir,
            inbox_dir,
            seen_dir,
            data: RwLock::new(data),
        })
    }

    pub async fn queue_message(&self, envelope: Envelope, ingress_peer: Option<String>) -> Result<bool> {
        let message_id = envelope.message_id;
        {
            let data = self.data.read().await;
            if data.durable_seen.contains(&message_id) || data.transient_seen.contains(&message_id) {
                return Ok(false);
            }
        }

        let message = StoredMessage {
            envelope,
            ingress_peer,
            acked_peers: BTreeSet::new(),
            last_attempt_ms_by_peer: BTreeMap::new(),
            stored_at_ms: now_ms(),
        };

        if message.envelope.traffic_class.should_persist_queue() {
            self.persist_seen(message_id).await?;
            self.persist_message(&message).await?;

            let mut data = self.data.write().await;
            data.durable_seen.insert(message_id);
            data.messages.insert(message_id, message);
        } else {
            let mut data = self.data.write().await;
            remember_transient_seen(&mut data, message_id);
            data.messages.insert(message_id, message);
        }
        Ok(true)
    }

    pub async fn store_inbox(&self, envelope: Envelope) -> Result<bool> {
        let message_id = envelope.message_id;
        {
            let data = self.data.read().await;
            if data.inbox.contains_key(&message_id) {
                return Ok(false);
            }
        }

        let item = InboxEntry {
            envelope,
            received_at_ms: now_ms(),
        };

        if item.envelope.traffic_class.should_persist_inbox() {
            self.persist_inbox_item(&item).await?;
        }

        let mut data = self.data.write().await;
        if item.envelope.traffic_class.should_persist_inbox() {
            data.durable_seen.insert(message_id);
        } else {
            remember_transient_seen(&mut data, message_id);
        }
        data.inbox.insert(message_id, item);
        Ok(true)
    }

    pub async fn mark_ack(&self, message_id: Uuid, peer_id: &str) -> Result<()> {
        let updated = {
            let mut data = self.data.write().await;
            if let Some(message) = data.messages.get_mut(&message_id) {
                message.acked_peers.insert(peer_id.to_string());
                Some(message.clone())
            } else {
                None
            }
        };

        if let Some(message) = updated {
            if message.envelope.traffic_class.should_persist_queue() {
                self.persist_message(&message).await?;
            }
        }
        Ok(())
    }

    pub async fn record_attempt(&self, message_id: Uuid, peer_id: &str) -> Result<()> {
        let updated = {
            let mut data = self.data.write().await;
            if let Some(message) = data.messages.get_mut(&message_id) {
                message
                    .last_attempt_ms_by_peer
                    .insert(peer_id.to_string(), now_ms());
                Some(message.clone())
            } else {
                None
            }
        };

        if let Some(message) = updated {
            if message.envelope.traffic_class.should_persist_queue() {
                self.persist_message(&message).await?;
            }
        }
        Ok(())
    }

    pub async fn mark_dispatched(&self, message_id: Uuid, peer_id: &str) -> Result<()> {
        let updated = {
            let mut data = self.data.write().await;
            if let Some(message) = data.messages.get_mut(&message_id) {
                message.acked_peers.insert(peer_id.to_string());
                Some(message.clone())
            } else {
                None
            }
        };

        if let Some(message) = updated {
            if message.envelope.traffic_class.should_persist_queue() {
                self.persist_message(&message).await?;
            }
        }
        Ok(())
    }

    pub async fn pending_messages(&self) -> Vec<StoredMessage> {
        let data = self.data.read().await;
        let mut items = data.messages.values().cloned().collect::<Vec<_>>();
        items.sort_by_key(|item| (item.envelope.traffic_class.dispatch_priority(), item.envelope.created_at_ms));
        items
    }

    pub async fn list_inbox(&self) -> Vec<InboxEntry> {
        let data = self.data.read().await;
        let mut items = data.inbox.values().cloned().collect::<Vec<_>>();
        items.sort_by_key(|item| std::cmp::Reverse(item.received_at_ms));
        items
    }

    pub async fn get_inbox(&self, message_id: Uuid) -> Option<InboxEntry> {
        let data = self.data.read().await;
        data.inbox.get(&message_id).cloned()
    }

    pub async fn counts(&self) -> (usize, usize) {
        let data = self.data.read().await;
        (data.messages.len(), data.inbox.len())
    }

    pub async fn remove_message(&self, message_id: Uuid) -> Result<bool> {
        let removed = {
            let mut data = self.data.write().await;
            data.messages.remove(&message_id)
        };

        if let Some(message) = removed {
            if message.envelope.traffic_class.should_persist_queue() {
                let path = self.message_path(message_id);
                let _ = tokio::fs::remove_file(path).await;
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub async fn cleanup_queue(&self, retention_ms: u64) -> Result<usize> {
        let cutoff = now_ms().saturating_sub(retention_ms);
        let expired = {
            let data = self.data.read().await;
            data.messages
                .values()
                .filter(|message| message.stored_at_ms <= cutoff)
                .map(|message| message.envelope.message_id)
                .collect::<Vec<_>>()
        };

        for message_id in &expired {
            if let Some(message) = self.data.read().await.messages.get(message_id).cloned() {
                if message.envelope.traffic_class.should_persist_queue() {
                    let path = self.message_path(*message_id);
                    let _ = tokio::fs::remove_file(path).await;
                }
            }
        }

        let mut data = self.data.write().await;
        for message_id in &expired {
            data.messages.remove(message_id);
        }
        Ok(expired.len())
    }

    async fn persist_message(&self, message: &StoredMessage) -> Result<()> {
        let bytes = bincode::serialize(message).context("failed to serialize queued message")?;
        tokio::fs::write(self.message_path(message.envelope.message_id), bytes)
            .await
            .context("failed to persist queued message")?;
        Ok(())
    }

    async fn persist_inbox_item(&self, item: &InboxEntry) -> Result<()> {
        let bytes = bincode::serialize(item).context("failed to serialize inbox entry")?;
        tokio::fs::write(self.inbox_path(item.envelope.message_id), bytes)
            .await
            .context("failed to persist inbox entry")?;
        Ok(())
    }

    async fn persist_seen(&self, message_id: Uuid) -> Result<()> {
        tokio::fs::write(self.seen_path(message_id), b"")
            .await
            .context("failed to persist seen marker")?;
        Ok(())
    }

    fn message_path(&self, message_id: Uuid) -> PathBuf {
        self.queue_dir.join(format!("{message_id}.bin"))
    }

    fn inbox_path(&self, message_id: Uuid) -> PathBuf {
        self.inbox_dir.join(format!("{message_id}.bin"))
    }

    fn seen_path(&self, message_id: Uuid) -> PathBuf {
        self.seen_dir.join(format!("{message_id}.seen"))
    }
}

async fn load_bin_directory<T>(dir: &Path) -> Result<Vec<T>>
where
    T: for<'de> Deserialize<'de>,
{
    let mut items = Vec::new();
    let mut read_dir = tokio::fs::read_dir(dir)
        .await
        .with_context(|| format!("failed to read directory {}", dir.display()))?;

    while let Some(entry) = read_dir.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|item| item.to_str()) != Some("bin") {
            continue;
        }
        let bytes = tokio::fs::read(&path)
            .await
            .with_context(|| format!("failed to read {}", path.display()))?;
        let item = bincode::deserialize::<T>(&bytes)
            .with_context(|| format!("failed to deserialize {}", path.display()))?;
        items.push(item);
    }

    Ok(items)
}

fn remember_transient_seen(data: &mut StorageData, message_id: Uuid) {
    if data.transient_seen.insert(message_id) {
        data.transient_seen_order.push_back(message_id);
    }
    while data.transient_seen_order.len() > MAX_TRANSIENT_SEEN {
        if let Some(expired) = data.transient_seen_order.pop_front() {
            data.transient_seen.remove(&expired);
        }
    }
}

async fn load_seen_directory(dir: &Path) -> Result<HashSet<Uuid>> {
    let mut items = HashSet::new();
    let mut read_dir = tokio::fs::read_dir(dir)
        .await
        .with_context(|| format!("failed to read directory {}", dir.display()))?;

    while let Some(entry) = read_dir.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|item| item.to_str()) != Some("seen") {
            continue;
        }
        if let Some(stem) = path.file_stem().and_then(|item| item.to_str()) {
            if let Ok(uuid) = Uuid::parse_str(stem) {
                items.insert(uuid);
            }
        }
    }

    Ok(items)
}
