use crate::config::AppConfig;
use crate::protocol::{
    now_ms, AckFrame, DeliveryTarget, Envelope, Frame, HelloFrame, PingFrame,
    StreamChunkFrame, StreamCloseFrame, StreamOpenFrame, SubscribeFrame, TrafficClass, TransportKind,
    UnsubscribeFrame,
};
use crate::storage::Storage;
use anyhow::{anyhow, Context, Result};
use dashmap::DashMap;
use serde::Serialize;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, RwLock};
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

#[derive(Clone)]
pub struct Runtime {
    config: Arc<AppConfig>,
    storage: Arc<Storage>,
    peers: Arc<PeerRegistry>,
    /// peer_id -> set of subscribed topics
    peer_subscriptions: Arc<DashMap<String, Vec<String>>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RuntimeStatus {
    pub node_id: String,
    pub tags: Vec<String>,
    pub pending_queue: usize,
    pub inbox_items: usize,
    pub peers: Vec<PeerStatus>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PeerStatus {
    pub node_id: String,
    pub tags: Vec<String>,
    pub links: Vec<PeerLinkStatus>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PeerLinkStatus {
    pub transport: String,
    pub remote_addr: String,
    pub connected_at_ms: u64,
}

#[derive(Default)]
pub struct PeerRegistry {
    inner: RwLock<HashMap<String, PeerLinks>>,
}

#[derive(Default)]
struct PeerLinks {
    usb: Option<LinkState>,
    wifi: Option<LinkState>,
}

#[derive(Clone)]
struct LinkState {
    connection_id: Uuid,
    sender: LinkSender,
    remote_addr: String,
    connected_at_ms: u64,
}

#[derive(Clone)]
struct LinkSender {
    control_tx: mpsc::Sender<Frame>,
    bulk_tx: mpsc::Sender<Frame>,
}

impl Runtime {
    pub async fn new(config: AppConfig) -> Result<Arc<Self>> {
        let storage = Arc::new(Storage::open(config.storage.root.clone()).await?);
        Ok(Arc::new(Self {
            config: Arc::new(config),
            storage,
            peers: Arc::new(PeerRegistry::default()),
            peer_subscriptions: Arc::new(DashMap::new()),
        }))
    }

    pub async fn start(self: Arc<Self>) -> Result<()> {
        if let Some(addr) = self.config.transport.usb_listen.clone() {
            let runtime = self.clone();
            tokio::spawn(async move {
                if let Err(error) = runtime.listen_loop(addr, TransportKind::Usb).await {
                    error!(%error, "usb listener stopped");
                }
            });
        }

        if let Some(addr) = self.config.transport.wifi_listen.clone() {
            let runtime = self.clone();
            tokio::spawn(async move {
                if let Err(error) = runtime.listen_loop(addr, TransportKind::Wifi).await {
                    error!(%error, "wifi listener stopped");
                }
            });
        }

        for peer in self.config.peers.clone() {
            if let Some(addr) = peer.usb_addr.clone() {
                let runtime = self.clone();
                let peer_id = peer.node_id.clone();
                tokio::spawn(async move {
                    runtime.connector_loop(peer_id, addr, TransportKind::Usb).await;
                });
            }

            if let Some(addr) = peer.wifi_addr.clone() {
                let runtime = self.clone();
                let peer_id = peer.node_id.clone();
                tokio::spawn(async move {
                    runtime.connector_loop(peer_id, addr, TransportKind::Wifi).await;
                });
            }
        }

        {
            let runtime = self.clone();
            tokio::spawn(async move {
                runtime.dispatch_loop().await;
            });
        }

        {
            let runtime = self.clone();
            tokio::spawn(async move {
                runtime.cleanup_loop().await;
            });
        }

        Ok(())
    }

    /// topic を指定したメッセージ送信（telemetry / stream 向け便利 API）
    #[allow(dead_code)]
    pub async fn publish(
        &self,
        topic: impl Into<String>,
        traffic_class: TrafficClass,
        content_type: String,
        payload: Vec<u8>,
    ) -> Result<Envelope> {
        let topic = topic.into();
        self.submit_message(
            DeliveryTarget::Broadcast,
            traffic_class,
            topic.clone(),
            topic,
            content_type,
            payload,
            None,
        )
        .await
    }

    /// chunked stream 送信: payload を chunk_size に分割して StreamOpen/StreamChunk/StreamClose を送信する
    /// chunk_size のデフォルト推奨値: 60_000 (60 KiB)
    #[allow(dead_code)]
    pub async fn publish_stream(
        &self,
        topic: impl Into<String>,
        content_type: String,
        payload: Vec<u8>,
        chunk_size: usize,
    ) -> Result<Uuid> {
        let topic = topic.into();
        let stream_id = Uuid::new_v4();
        let chunk_size = chunk_size.max(1);
        // owned chunks を先に構築してライフタイム問題を回避
        let owned_chunks: Vec<Vec<u8>> = payload.chunks(chunk_size).map(|c| c.to_vec()).collect();
        let total_chunks = owned_chunks.len() as u32;
        let total_bytes = payload.len() as u64;

        let open = StreamOpenFrame {
            stream_id,
            source_node: self.node_id().to_string(),
            topic: topic.clone(),
            content_type,
            total_chunks,
            total_bytes,
            timestamp_ms: now_ms(),
        };

        // 接続中 peer の sender を snapshot してから送信（途中の接続変化に安全）
        let senders = self.peers.all_senders().await;
        for sender in &senders {
            let _ = sender.send(Frame::StreamOpen(open.clone())).await;
        }

        for (index, chunk_data) in owned_chunks.iter().enumerate() {
            let chunk = StreamChunkFrame {
                stream_id,
                chunk_index: index as u32,
                data: chunk_data.clone(),
            };
            for sender in &senders {
                let _ = sender.send(Frame::StreamChunk(chunk.clone())).await;
            }
        }

        let close = StreamCloseFrame {
            stream_id,
            timestamp_ms: now_ms(),
        };
        for sender in &senders {
            let _ = sender.send(Frame::StreamClose(close.clone())).await;
        }

        Ok(stream_id)
    }

    pub async fn submit_message(
        &self,
        target: DeliveryTarget,
        traffic_class: TrafficClass,
        topic: String,
        subject: String,
        content_type: String,
        payload: Vec<u8>,
        ttl: Option<u8>,
    ) -> Result<Envelope> {
        let envelope = Envelope {
            message_id: Uuid::new_v4(),
            source_node: self.node_id().to_string(),
            target,
            traffic_class,
            topic,
            subject,
            content_type,
            created_at_ms: now_ms(),
            ttl: ttl.unwrap_or(self.config.transport.max_hops),
            hops: 0,
            payload,
        };

        self.storage.queue_message(envelope.clone(), None).await?;
        if envelope.traffic_class.should_store_inbox() && envelope.target.matches(self.node_id()) {
            self.storage.store_inbox(envelope.clone()).await?;
        }
        if matches!(envelope.traffic_class, TrafficClass::Telemetry) && !envelope.topic.is_empty() {
            self.storage.update_topic(envelope.clone()).await;
        }
        Ok(envelope)
    }

    pub async fn accept_remote_message(&self, envelope: Envelope, ingress_peer: Option<String>) -> Result<bool> {
        let is_new = self.storage.queue_message(envelope.clone(), ingress_peer).await?;
        if envelope.traffic_class.should_store_inbox() && envelope.target.matches(self.node_id()) {
            self.storage.store_inbox(envelope.clone()).await?;
        }
        if matches!(envelope.traffic_class, TrafficClass::Telemetry) && !envelope.topic.is_empty() {
            self.storage.update_topic(envelope).await;
        }
        Ok(is_new)
    }

    /// peer が subscribe しているトピック一覧を返す
    #[allow(dead_code)]
    pub fn peer_topics(&self, peer_id: &str) -> Vec<String> {
        self.peer_subscriptions
            .get(peer_id)
            .map(|r| r.clone())
            .unwrap_or_default()
    }

    /// storage への公開アクセス（web.rs から使う）
    pub fn storage(&self) -> Arc<Storage> {
        self.storage.clone()
    }

    pub async fn status_snapshot(&self) -> RuntimeStatus {
        let (pending_queue, inbox_items) = self.storage.counts().await;
        let link_map = self.peers.snapshot().await;
        let mut peer_map = HashMap::<String, PeerStatus>::new();

        for peer in &self.config.peers {
            peer_map.insert(
                peer.node_id.clone(),
                PeerStatus {
                    node_id: peer.node_id.clone(),
                    tags: peer.tags.clone(),
                    links: Vec::new(),
                },
            );
        }

        for (node_id, links) in link_map {
            let tags = self
                .config
                .peers
                .iter()
                .find(|peer| peer.node_id == node_id)
                .map(|peer| peer.tags.clone())
                .unwrap_or_default();
            peer_map
                .entry(node_id.clone())
                .and_modify(|peer| peer.links = links.clone())
                .or_insert(PeerStatus { node_id, tags, links });
        }

        let mut peers = peer_map.into_values().collect::<Vec<_>>();
        peers.sort_by(|left, right| left.node_id.cmp(&right.node_id));

        RuntimeStatus {
            node_id: self.node_id().to_string(),
            tags: self.config.node.tags.clone(),
            pending_queue,
            inbox_items,
            peers,
        }
    }

    pub fn node_id(&self) -> &str {
        &self.config.node.node_id
    }

    pub fn web_bind_addr(&self) -> Result<SocketAddr> {
        self.config
            .web
            .bind
            .parse()
            .with_context(|| format!("invalid web bind address: {}", self.config.web.bind))
    }

    async fn listen_loop(self: Arc<Self>, addr: String, transport: TransportKind) -> Result<()> {
        let listener = TcpListener::bind(&addr)
            .await
            .with_context(|| format!("failed to bind listener on {addr}"))?;
        info!(%addr, ?transport, "listener started");

        loop {
            let (stream, remote_addr) = listener.accept().await.context("listener accept failed")?;
            let runtime = self.clone();
            let transport_kind = transport.clone();
            tokio::spawn(async move {
                if let Err(error) = runtime.run_connection(stream, transport_kind, None).await {
                    warn!(%error, %remote_addr, "connection closed with error");
                }
            });
        }
    }

    async fn run_connection(
        self: Arc<Self>,
        stream: TcpStream,
        transport: TransportKind,
        expected_peer: Option<String>,
    ) -> Result<()> {
        let remote_addr = stream
            .peer_addr()
            .map(|addr| addr.to_string())
            .unwrap_or_else(|_| "unknown".to_string());
        let (mut reader, mut writer) = stream.into_split();

        write_frame(
            &mut writer,
            &Frame::Hello(HelloFrame {
                node_id: self.node_id().to_string(),
                transport: transport.clone(),
                capabilities: vec!["reliable-queue".to_string(), "webui".to_string(), "mesh-flood".to_string()],
                timestamp_ms: now_ms(),
            }),
        )
        .await?;

        let remote_hello = match read_frame(&mut reader).await? {
            Frame::Hello(hello) => hello,
            frame => return Err(anyhow!("expected hello frame, got {frame:?}")),
        };

        if let Some(expected_peer) = expected_peer.as_ref() {
            if expected_peer != &remote_hello.node_id {
                warn!(expected = %expected_peer, actual = %remote_hello.node_id, "peer identity mismatch");
            }
        }

        let peer_id = remote_hello.node_id.clone();
        let connection_id = Uuid::new_v4();
        let (control_tx, mut control_rx) = mpsc::channel::<Frame>(512);
        let (bulk_tx, mut bulk_rx) = mpsc::channel::<Frame>(1024);
        let tx = LinkSender {
            control_tx: control_tx.clone(),
            bulk_tx: bulk_tx.clone(),
        };

        let writer_task = tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    Some(frame) = control_rx.recv() => {
                        write_frame(&mut writer, &frame).await?;
                    }
                    Some(frame) = bulk_rx.recv() => {
                        write_frame(&mut writer, &frame).await?;
                    }
                    else => break,
                }
            }
            Ok::<(), anyhow::Error>(())
        });

        self.peers
            .register(peer_id.clone(), transport.clone(), connection_id, tx.clone(), remote_addr.clone())
            .await;

        let heartbeat_tx = tx.clone();
        let heartbeat_interval = Duration::from_millis(self.config.transport.heartbeat_interval_ms);
        let heartbeat_task = tokio::spawn(async move {
            loop {
                sleep(heartbeat_interval).await;
                if heartbeat_tx
                    .send(Frame::Ping(PingFrame { timestamp_ms: now_ms() }))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });

        info!(peer_id = %peer_id, %remote_addr, ?transport, "peer registered");

        let result = self
            .clone()
            .read_connection_loop(&mut reader, &peer_id, &tx)
            .await;

        heartbeat_task.abort();
        self.peers.unregister(&peer_id, &transport, connection_id).await;
        drop(control_tx);
        drop(bulk_tx);

        match writer_task.await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => warn!(%error, peer_id = %peer_id, "writer task ended with error"),
            Err(error) => warn!(%error, peer_id = %peer_id, "writer task join error"),
        }

        result
    }

    async fn read_connection_loop(
        self: Arc<Self>,
        reader: &mut OwnedReadHalf,
        peer_id: &str,
        writer_tx: &LinkSender,
    ) -> Result<()> {
        loop {
            match read_frame(reader).await {
                Ok(Frame::Data(envelope)) => {
                    let requires_ack = envelope.traffic_class.requires_ack();
                    let message_id = envelope.message_id;
                    if let Err(error) = self.accept_remote_message(envelope, Some(peer_id.to_string())).await {
                        warn!(%error, peer_id = %peer_id, %message_id, "failed to accept remote message");
                    }
                    if requires_ack {
                        writer_tx
                            .send(Frame::Ack(AckFrame {
                                message_id,
                                from_node: self.node_id().to_string(),
                                timestamp_ms: now_ms(),
                            }))
                            .await
                            .context("failed to send ack")?;
                    }
                }
                Ok(Frame::Ack(ack)) => {
                    self.storage.mark_ack(ack.message_id, peer_id).await?;
                }
                Ok(Frame::Ping(ping)) => {
                    writer_tx.send(Frame::Pong(ping)).await.context("failed to send pong")?;
                }
                Ok(Frame::Pong(_)) => {}
                Ok(Frame::Hello(_)) => {}
                Ok(Frame::Subscribe(sub)) => {
                    self.handle_subscribe(peer_id, sub).await;
                }
                Ok(Frame::Unsubscribe(unsub)) => {
                    self.handle_unsubscribe(peer_id, unsub).await;
                }
                Ok(Frame::StreamOpen(open)) => {
                    self.storage.stream_open(open).await;
                }
                Ok(Frame::StreamChunk(chunk)) => {
                    if let Some(assembled) = self.storage.stream_chunk(chunk).await {
                        self.storage.stream_close(assembled).await;
                    }
                }
                Ok(Frame::StreamClose(close)) => {
                    // StreamClose を受け取ったら、chunk が揃っていなくても強制的に組み立てる
                    // (パケロスや片道切断でも受信側が詰まらないようにする)
                    if let Some(assembled) = self.storage.stream_force_close(close.stream_id).await {
                        self.storage.stream_close(assembled).await;
                    }
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn handle_subscribe(&self, peer_id: &str, sub: SubscribeFrame) {
        let mut entry = self.peer_subscriptions.entry(peer_id.to_string()).or_default();
        for topic in sub.topics {
            if !entry.contains(&topic) {
                entry.push(topic);
            }
        }
        debug!(peer_id = %peer_id, "subscribe updated");
    }

    async fn handle_unsubscribe(&self, peer_id: &str, unsub: UnsubscribeFrame) {
        if let Some(mut entry) = self.peer_subscriptions.get_mut(peer_id) {
            entry.retain(|t| !unsub.topics.contains(t));
        }
    }

    async fn connector_loop(self: Arc<Self>, peer_id: String, addr: String, transport: TransportKind) {
        let interval = Duration::from_millis(self.config.transport.connect_interval_ms);
        loop {
            if self.peers.has_link(&peer_id, &transport).await {
                sleep(interval).await;
                continue;
            }

            match TcpStream::connect(&addr).await {
                Ok(stream) => {
                    info!(peer_id = %peer_id, %addr, ?transport, "outbound transport connected");
                    if let Err(error) = self.clone().run_connection(stream, transport.clone(), Some(peer_id.clone())).await {
                        warn!(%error, peer_id = %peer_id, %addr, ?transport, "outbound connection ended");
                    }
                }
                Err(error) => {
                    debug!(%error, peer_id = %peer_id, %addr, ?transport, "connect attempt failed");
                }
            }

            sleep(interval).await;
        }
    }

    async fn dispatch_loop(self: Arc<Self>) {
        let tick = Duration::from_millis(25);
        let ack_timeout_ms = self.config.transport.ack_timeout_ms;

        loop {
            let pending = self.storage.pending_messages().await;
            let connected_peers = self.peers.connected_peer_ids().await;
            // transient メッセージで即時削除するものを収集してまとめて削除する
            // （ループ中に remove すると次 tick でもまだ pending に残るが、
            //   dispatched フラグで二重送信は防ぐ。削除は tick 後に実施）
            let mut to_remove: Vec<uuid::Uuid> = Vec::new();

            for message in pending {
                if message.envelope.hops >= message.envelope.ttl {
                    if !message.envelope.traffic_class.requires_ack() {
                        to_remove.push(message.envelope.message_id);
                    }
                    continue;
                }
                if !message.envelope.target.requires_forwarding_after(self.node_id()) {
                    if !message.envelope.traffic_class.requires_ack() {
                        to_remove.push(message.envelope.message_id);
                    }
                    continue;
                }

                let mut candidates = connected_peers.clone();
                sort_candidates_for_message(&mut candidates, &message.envelope.target);

                let mut dispatched_any = false;
                for peer_id in candidates {
                    if peer_id == self.node_id() {
                        continue;
                    }
                    if message.ingress_peer.as_deref() == Some(peer_id.as_str()) {
                        continue;
                    }
                    if message.acked_peers.contains(&peer_id) {
                        continue;
                    }

                    let should_retry = if message.envelope.traffic_class.requires_ack() {
                        match message.last_attempt_ms_by_peer.get(&peer_id) {
                            Some(last_attempt_ms) => now_ms().saturating_sub(*last_attempt_ms) >= ack_timeout_ms,
                            None => true,
                        }
                    } else {
                        // transient: まだ dispatched されていない peer にのみ送る
                        !message.acked_peers.contains(&peer_id)
                    };

                    if !should_retry {
                        continue;
                    }

                    if let Some(sender) = self.peers.best_sender(&peer_id).await {
                        let mut outbound = message.envelope.clone();
                        outbound.hops = outbound.hops.saturating_add(1);
                        if outbound.hops > outbound.ttl {
                            continue;
                        }

                        match sender.send(Frame::Data(outbound)).await {
                            Ok(()) => {
                                dispatched_any = true;
                                let store_result = if message.envelope.traffic_class.requires_ack() {
                                    self.storage.record_attempt(message.envelope.message_id, &peer_id).await
                                } else {
                                    self.storage.mark_dispatched(message.envelope.message_id, &peer_id).await
                                };
                                if let Err(error) = store_result {
                                    warn!(%error, peer_id = %peer_id, "failed to update dispatch state");
                                }
                            }
                            Err(error) => {
                                warn!(%error, peer_id = %peer_id, "failed to dispatch frame");
                            }
                        }
                    }
                }

                // transient メッセージ: 全候補に送り終えたら削除
                if !message.envelope.traffic_class.requires_ack() && dispatched_any {
                    to_remove.push(message.envelope.message_id);
                }
            }

            for message_id in to_remove {
                if let Err(error) = self.storage.remove_message(message_id).await {
                    warn!(%error, %message_id, "failed to remove transient message");
                }
            }

            sleep(tick).await;
        }
    }

    async fn cleanup_loop(self: Arc<Self>) {
        let interval = Duration::from_secs(30);
        let retention_ms = self.config.storage.retention_seconds.saturating_mul(1_000);

        loop {
            if let Err(error) = self.storage.cleanup_queue(retention_ms).await {
                warn!(%error, "queue cleanup failed");
            }
            self.storage.cleanup_streams().await;
            sleep(interval).await;
        }
    }
}

impl PeerRegistry {
    async fn register(
        &self,
        peer_id: String,
        transport: TransportKind,
        connection_id: Uuid,
        sender: LinkSender,
        remote_addr: String,
    ) {
        let mut inner = self.inner.write().await;
        let entry = inner.entry(peer_id).or_default();
        let link = LinkState {
            connection_id,
            sender,
            remote_addr,
            connected_at_ms: now_ms(),
        };
        match transport {
            TransportKind::Usb => entry.usb = Some(link),
            TransportKind::Wifi => entry.wifi = Some(link),
        }
    }

    async fn unregister(&self, peer_id: &str, transport: &TransportKind, connection_id: Uuid) {
        let mut inner = self.inner.write().await;
        let mut remove_peer = false;
        if let Some(entry) = inner.get_mut(peer_id) {
            let remove = match transport {
                TransportKind::Usb => entry.usb.as_ref().is_some_and(|link| link.connection_id == connection_id),
                TransportKind::Wifi => entry.wifi.as_ref().is_some_and(|link| link.connection_id == connection_id),
            };
            if remove {
                match transport {
                    TransportKind::Usb => entry.usb = None,
                    TransportKind::Wifi => entry.wifi = None,
                }
            }
            if entry.usb.is_none() && entry.wifi.is_none() {
                remove_peer = true;
            }
        }
        if remove_peer {
            inner.remove(peer_id);
        }
    }

    async fn has_link(&self, peer_id: &str, transport: &TransportKind) -> bool {
        let inner = self.inner.read().await;
        inner.get(peer_id).is_some_and(|entry| match transport {
            TransportKind::Usb => entry.usb.is_some(),
            TransportKind::Wifi => entry.wifi.is_some(),
        })
    }

    async fn best_sender(&self, peer_id: &str) -> Option<LinkSender> {
        let inner = self.inner.read().await;
        let entry = inner.get(peer_id)?;
        if let Some(link) = &entry.usb {
            return Some(link.sender.clone());
        }
        if let Some(link) = &entry.wifi {
            return Some(link.sender.clone());
        }
        None
    }

    async fn connected_peer_ids(&self) -> Vec<String> {
        let inner = self.inner.read().await;
        inner.keys().cloned().collect()
    }

    /// 全接続中 peer の best sender を snapshot で返す（publish_stream 用）
    async fn all_senders(&self) -> Vec<LinkSender> {
        let inner = self.inner.read().await;
        inner
            .values()
            .filter_map(|links| {
                links.usb.as_ref().map(|l| l.sender.clone())
                    .or_else(|| links.wifi.as_ref().map(|l| l.sender.clone()))
            })
            .collect()
    }

    async fn snapshot(&self) -> HashMap<String, Vec<PeerLinkStatus>> {
        let inner = self.inner.read().await;
        inner
            .iter()
            .map(|(node_id, links)| {
                let mut statuses = Vec::new();
                if let Some(link) = &links.usb {
                    statuses.push(PeerLinkStatus {
                        transport: "usb".to_string(),
                        remote_addr: link.remote_addr.clone(),
                        connected_at_ms: link.connected_at_ms,
                    });
                }
                if let Some(link) = &links.wifi {
                    statuses.push(PeerLinkStatus {
                        transport: "wifi".to_string(),
                        remote_addr: link.remote_addr.clone(),
                        connected_at_ms: link.connected_at_ms,
                    });
                }
                (node_id.clone(), statuses)
            })
            .collect()
    }
}

fn sort_candidates_for_message(candidates: &mut [String], target: &DeliveryTarget) {
    candidates.sort_by_key(|peer_id| peer_priority(peer_id, Some(target)));
}

fn peer_priority(peer_id: &str, target: Option<&DeliveryTarget>) -> u8 {
    match target {
        Some(DeliveryTarget::Node(node_id)) if node_id == peer_id => 0,
        Some(DeliveryTarget::Nodes(nodes)) if nodes.iter().any(|node_id| node_id == peer_id) => 0,
        _ => 1,
    }
}

async fn write_frame(writer: &mut OwnedWriteHalf, frame: &Frame) -> Result<()> {
    let bytes = bincode::serialize(frame).context("failed to serialize frame")?;
    writer
        .write_u32(bytes.len() as u32)
        .await
        .context("failed to write frame length")?;
    writer
        .write_all(&bytes)
        .await
        .context("failed to write frame bytes")?;
    writer.flush().await.context("failed to flush frame")?;
    Ok(())
}

/// フレームサイズ上限: 32 MiB (stream chunk 含む)
const MAX_FRAME_SIZE: u32 = 32 * 1024 * 1024;

async fn read_frame(reader: &mut OwnedReadHalf) -> Result<Frame> {
    let length = reader
        .read_u32()
        .await
        .context("failed to read frame length")?;
    if length == 0 {
        return Err(anyhow!("received zero-length frame"));
    }
    if length > MAX_FRAME_SIZE {
        return Err(anyhow!("frame too large: {} bytes (max {})", length, MAX_FRAME_SIZE));
    }
    let mut buffer = vec![0_u8; length as usize];
    reader
        .read_exact(&mut buffer)
        .await
        .context("failed to read frame body")?;
    bincode::deserialize(&buffer).context("failed to deserialize frame")
}

impl LinkSender {
    async fn send(&self, frame: Frame) -> Result<()> {
        let sender = match &frame {
            // 非 Control の Data フレームと stream 系フレームは bulk キュー
            Frame::Data(envelope) if !matches!(envelope.traffic_class, TrafficClass::Control) => &self.bulk_tx,
            Frame::StreamOpen(_) | Frame::StreamChunk(_) | Frame::StreamClose(_) => &self.bulk_tx,
            // 制御系・ACK・Ping/Pong・Subscribe は control キュー
            _ => &self.control_tx,
        };
        sender.send(frame).await.map_err(|error| anyhow!("failed to enqueue frame: {error}"))
    }
}
