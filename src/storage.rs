use crate::protocol::{now_ms, Envelope, StreamChunkFrame, StreamOpenFrame};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use tokio::sync::{broadcast, RwLock};
use uuid::Uuid;

const MAX_TRANSIENT_SEEN: usize = 65_536;
/// telemetry topic ごとに保持する最新値の上限数
const MAX_TOPIC_CACHE: usize = 4096;
/// stream ring buffer のチャンク上限
const MAX_STREAM_RING: usize = 8;

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

#[derive(Clone, Debug)]
pub struct TopicEntry {
    pub envelope: Envelope,
    pub received_at_ms: u64,
}

/// chunked stream の組み立て中状態
#[derive(Debug)]
pub struct InProgressStream {
    pub meta: StreamOpenFrame,
    pub chunks: Vec<Option<Vec<u8>>>,
    pub received_count: u32,
    pub started_at_ms: u64,
}

/// 組み立て完了した stream
#[derive(Clone, Debug)]
pub struct AssembledStream {
    pub meta: StreamOpenFrame,
    #[allow(dead_code)]
    pub data: Vec<u8>,
    pub completed_at_ms: u64,
}

#[derive(Default)]
struct StorageData {
    messages: HashMap<Uuid, StoredMessage>,
    inbox: HashMap<Uuid, InboxEntry>,
    durable_seen: HashSet<Uuid>,
    transient_seen: HashSet<Uuid>,
    transient_seen_order: VecDeque<Uuid>,
    /// telemetry 最新値: topic -> TopicEntry
    topic_cache: HashMap<String, TopicEntry>,
    topic_cache_order: VecDeque<String>,
}

pub struct Storage {
    queue_dir: PathBuf,
    inbox_dir: PathBuf,
    seen_dir: PathBuf,
    data: RwLock<StorageData>,
    /// stream 組み立てバッファ: stream_id -> InProgressStream
    stream_buf: RwLock<HashMap<Uuid, InProgressStream>>,
    /// ring buffer: 完成した stream の最新 MAX_STREAM_RING 個
    stream_ring: RwLock<VecDeque<AssembledStream>>,
    /// topic 更新を WebSocket 側に通知する broadcast チャンネル
    pub topic_tx: broadcast::Sender<TopicEntry>,
    /// stream 完成を WebSocket 側に通知する broadcast チャンネル
    pub stream_tx: broadcast::Sender<AssembledStream>,
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

        let (topic_tx, _) = broadcast::channel(256);
        let (stream_tx, _) = broadcast::channel(32);

        Ok(Self {
            queue_dir,
            inbox_dir,
            seen_dir,
            data: RwLock::new(data),
            stream_buf: RwLock::new(HashMap::new()),
            stream_ring: RwLock::new(VecDeque::new()),
            topic_tx,
            stream_tx,
        })
    }

    /// telemetry メッセージを topic キャッシュに登録し、subscriber へ通知する
    pub async fn update_topic(&self, envelope: Envelope) {
        let topic = envelope.topic.clone();
        if topic.is_empty() {
            return;
        }
        let entry = TopicEntry {
            envelope,
            received_at_ms: now_ms(),
        };
        {
            let mut data = self.data.write().await;
            if !data.topic_cache.contains_key(&topic) {
                data.topic_cache_order.push_back(topic.clone());
                while data.topic_cache_order.len() > MAX_TOPIC_CACHE {
                    if let Some(old) = data.topic_cache_order.pop_front() {
                        data.topic_cache.remove(&old);
                    }
                }
            }
            data.topic_cache.insert(topic, entry.clone());
        }
        let _ = self.topic_tx.send(entry);
    }

    /// topic の最新値を取得する
    pub async fn get_topic(&self, topic: &str) -> Option<TopicEntry> {
        let data = self.data.read().await;
        data.topic_cache.get(topic).cloned()
    }

    /// 全 topic の最新値一覧を返す
    pub async fn list_topics(&self) -> Vec<TopicEntry> {
        let data = self.data.read().await;
        let mut items = data.topic_cache.values().cloned().collect::<Vec<_>>();
        items.sort_by(|a, b| a.envelope.topic.cmp(&b.envelope.topic));
        items
    }

    /// chunked stream の開始を登録する
    pub async fn stream_open(&self, meta: StreamOpenFrame) {
        let stream_id = meta.stream_id;
        let total = meta.total_chunks as usize;
        let entry = InProgressStream {
            meta,
            chunks: vec![None; total],
            received_count: 0,
            started_at_ms: now_ms(),
        };
        self.stream_buf.write().await.insert(stream_id, entry);
    }

    /// チャンクを追加し、完成したら AssembledStream を返す
    pub async fn stream_chunk(&self, chunk: StreamChunkFrame) -> Option<AssembledStream> {
        let mut buf = self.stream_buf.write().await;
        let entry = buf.get_mut(&chunk.stream_id)?;
        let idx = chunk.chunk_index as usize;
        if idx < entry.chunks.len() && entry.chunks[idx].is_none() {
            entry.chunks[idx] = Some(chunk.data);
            entry.received_count += 1;
        }
        if entry.received_count == entry.meta.total_chunks {
            let assembled_data: Vec<u8> = entry.chunks.iter().flatten().flatten().cloned().collect();
            let assembled = AssembledStream {
                meta: entry.meta.clone(),
                data: assembled_data,
                completed_at_ms: now_ms(),
            };
            buf.remove(&chunk.stream_id);
            Some(assembled)
        } else {
            None
        }
    }

    /// 組み立て完了した stream を ring buffer に追加し subscriber へ通知する
    pub async fn stream_close(&self, assembled: AssembledStream) {
        {
            let mut ring = self.stream_ring.write().await;
            ring.push_back(assembled.clone());
            while ring.len() > MAX_STREAM_RING {
                ring.pop_front();
            }
        }
        let _ = self.stream_tx.send(assembled);
    }

    /// stream ring buffer の内容を返す
    pub async fn list_streams(&self) -> Vec<AssembledStream> {
        self.stream_ring.read().await.iter().cloned().collect()
    }

    /// StreamClose を受け取ったとき、chunk が揃っていなくても強制的に組み立てる
    /// 揃っていない chunk は空 Vec で補完する（パケロス耐性）
    pub async fn stream_force_close(&self, stream_id: Uuid) -> Option<AssembledStream> {
        let mut buf = self.stream_buf.write().await;
        let entry = buf.remove(&stream_id)?;
        let assembled_data: Vec<u8> = entry
            .chunks
            .into_iter()
            .flat_map(|c| c.unwrap_or_default())
            .collect();
        Some(AssembledStream {
            meta: entry.meta,
            data: assembled_data,
            completed_at_ms: now_ms(),
        })
    }

    /// 未完成 stream のタイムアウトをクリーンアップ（30 秒超）
    pub async fn cleanup_streams(&self) {
        let cutoff = now_ms().saturating_sub(30_000);
        self.stream_buf.write().await.retain(|_, v| v.started_at_ms > cutoff);
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
