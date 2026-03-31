use crate::config::AppConfig;
use crate::protocol::{
    now_ms, AckFrame, DeliveryTarget, Envelope, Frame, HelloFrame, PingFrame,
    StreamChunkFrame, StreamCloseFrame, StreamOpenFrame, SubscribeFrame, TrafficClass, TransportKind,
    UnsubscribeFrame,
};
use crate::storage::Storage;
use crate::wifi;
use anyhow::{anyhow, Context, Result};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{SocketAddr, SocketAddrV4};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
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
    /// discovery で検出した peer 情報
    discovered_peers: Arc<DashMap<String, DiscoveredPeer>>,
    /// network connector loop を peer ごとに 1 本だけ起動するためのガード
    network_connector_workers: Arc<DashMap<String, ()>>,
    peer_policies: Arc<DashMap<String, PeerPolicy>>,
    started_at_ms: u64,
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
    pub role: Option<String>,
    pub relationship: String,
    pub preferred_transport_order: Vec<String>,
    pub tags: Vec<String>,
    pub configured: bool,
    pub discovered: bool,
    pub alive: bool,
    pub power_state: String,
    pub last_seen_ms: Option<u64>,
    pub last_report_ms: Option<u64>,
    pub uptime_ms: Option<u64>,
    pub remote_pending_queue: Option<usize>,
    pub remote_inbox_items: Option<usize>,
    pub remote_wifi_mode: Option<String>,
    pub remote_wifi_ssid: Option<String>,
    pub remote_wifi_signal: Option<u8>,
    pub connection_quality: String,
    pub last_rtt_ms: Option<u64>,
    pub advertised_network_addr: Option<String>,
    pub advertised_web_url: Option<String>,
    pub links: Vec<PeerLinkStatus>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PeerLinkStatus {
    pub transport: String,
    pub remote_addr: String,
    pub connected_at_ms: u64,
    pub last_rx_ms: Option<u64>,
    pub last_rtt_ms: Option<u64>,
    pub quality: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct PeerPolicy {
    pub node_id: String,
    pub role: Option<String>,
    pub relationship: String,
    pub preferred_transport_order: Vec<String>,
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
    last_rx_ms: Option<u64>,
    last_rtt_ms: Option<u64>,
}

#[derive(Clone)]
struct LinkSender {
    control_tx: mpsc::Sender<Frame>,
    bulk_tx: mpsc::Sender<Frame>,
}

#[derive(Clone, Debug)]
struct DiscoveredPeer {
    network_addr: String,
    web_url: Option<String>,
    tags: Vec<String>,
    last_seen_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct DiscoveryPacket {
    version: u8,
    node_id: String,
    tags: Vec<String>,
    network_port: Option<u16>,
    web_port: Option<u16>,
    timestamp_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct NodeRuntimeReport {
    node_id: String,
    role: String,
    uptime_ms: u64,
    pending_queue: usize,
    inbox_items: usize,
    wifi_mode: String,
    wifi_ssid: Option<String>,
    wifi_signal: Option<u8>,
    power_state: String,
    timestamp_ms: u64,
}

impl Runtime {
    pub async fn new(config: AppConfig) -> Result<Arc<Self>> {
        let storage = Arc::new(Storage::open(config.storage.root.clone()).await?);
        let peer_policies = config
            .peers
            .iter()
            .map(|peer| {
                (
                    peer.node_id.clone(),
                    PeerPolicy {
                        node_id: peer.node_id.clone(),
                        role: peer.role.clone(),
                        relationship: peer.relationship.clone(),
                        preferred_transport_order: normalize_transport_order(&peer.preferred_transport_order),
                    },
                )
            })
            .collect::<DashMap<_, _>>();
        Ok(Arc::new(Self {
            config: Arc::new(config),
            storage,
            peers: Arc::new(PeerRegistry::default()),
            peer_subscriptions: Arc::new(DashMap::new()),
            discovered_peers: Arc::new(DashMap::new()),
            network_connector_workers: Arc::new(DashMap::new()),
            peer_policies: Arc::new(peer_policies),
            started_at_ms: now_ms(),
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

        if let Some(addr) = self.config.transport.network_listen.clone() {
            let runtime = self.clone();
            tokio::spawn(async move {
                if let Err(error) = runtime.listen_loop(addr, TransportKind::Wifi).await {
                    error!(%error, "network listener stopped");
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

            if self.config.discovery.enabled || peer.network_addr.is_some() {
                self.spawn_network_connector_loop(peer.node_id.clone());
            }
        }

        if self.config.discovery.enabled {
            {
                let runtime = self.clone();
                tokio::spawn(async move {
                    if let Err(error) = runtime.discovery_listener_loop().await {
                        error!(%error, "discovery listener stopped");
                    }
                });
            }
            {
                let runtime = self.clone();
                tokio::spawn(async move {
                    if let Err(error) = runtime.discovery_announce_loop().await {
                        error!(%error, "discovery announce loop stopped");
                    }
                });
            }
        }

        if self.config.wifi.auto_manage {
            let runtime = self.clone();
            tokio::spawn(async move {
                runtime.wifi_auto_manage_loop().await;
            });
        }

        {
            let runtime = self.clone();
            tokio::spawn(async move {
                runtime.node_status_report_loop().await;
            });
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

    pub async fn status_snapshot(&self) -> RuntimeStatus {
        let (pending_queue, inbox_items) = self.storage.counts().await;
        let link_map = self.peers.snapshot().await;
        let mut peer_map = HashMap::<String, PeerStatus>::new();

        for peer in &self.config.peers {
            let policy = self.peer_policy_for(&peer.node_id);
            peer_map.insert(
                peer.node_id.clone(),
                PeerStatus {
                    node_id: peer.node_id.clone(),
                    role: policy.role.clone(),
                    relationship: policy.relationship.clone(),
                    preferred_transport_order: policy.preferred_transport_order.clone(),
                    tags: peer.tags.clone(),
                    configured: true,
                    discovered: false,
                    alive: false,
                    power_state: "unknown".to_string(),
                    last_seen_ms: None,
                    last_report_ms: None,
                    uptime_ms: None,
                    remote_pending_queue: None,
                    remote_inbox_items: None,
                    remote_wifi_mode: None,
                    remote_wifi_ssid: None,
                    remote_wifi_signal: None,
                    connection_quality: "unknown".to_string(),
                    last_rtt_ms: None,
                    advertised_network_addr: peer.network_addr.clone(),
                    advertised_web_url: None,
                    links: Vec::new(),
                },
            );
        }

        for discovered in self.discovered_peers.iter() {
            let peer_id = discovered.key().clone();
            let info = discovered.value().clone();
            peer_map
                .entry(peer_id.clone())
                .and_modify(|peer| {
                    if peer.tags.is_empty() {
                        peer.tags = info.tags.clone();
                    }
                    peer.discovered = true;
                    peer.last_seen_ms = Some(info.last_seen_ms);
                    peer.advertised_network_addr = Some(info.network_addr.clone());
                    peer.advertised_web_url = info.web_url.clone();
                })
                .or_insert(PeerStatus {
                    node_id: peer_id,
                    role: None,
                    relationship: "peer".to_string(),
                    preferred_transport_order: default_transport_order(),
                    tags: info.tags,
                    configured: false,
                    discovered: true,
                    alive: false,
                    power_state: "unknown".to_string(),
                    last_seen_ms: Some(info.last_seen_ms),
                    last_report_ms: None,
                    uptime_ms: None,
                    remote_pending_queue: None,
                    remote_inbox_items: None,
                    remote_wifi_mode: None,
                    remote_wifi_ssid: None,
                    remote_wifi_signal: None,
                    connection_quality: "unknown".to_string(),
                    last_rtt_ms: None,
                    advertised_network_addr: Some(info.network_addr),
                    advertised_web_url: info.web_url,
                    links: Vec::new(),
                });
        }

        for (node_id, links) in link_map {
            peer_map
                .entry(node_id.clone())
                .and_modify(|peer| peer.links = links.clone())
                .or_insert(PeerStatus {
                    node_id,
                    role: None,
                    relationship: "peer".to_string(),
                    preferred_transport_order: default_transport_order(),
                    tags: Vec::new(),
                    configured: false,
                    discovered: false,
                    alive: false,
                    power_state: "unknown".to_string(),
                    last_seen_ms: None,
                    last_report_ms: None,
                    uptime_ms: None,
                    remote_pending_queue: None,
                    remote_inbox_items: None,
                    remote_wifi_mode: None,
                    remote_wifi_ssid: None,
                    remote_wifi_signal: None,
                    connection_quality: "unknown".to_string(),
                    last_rtt_ms: None,
                    advertised_network_addr: None,
                    advertised_web_url: None,
                    links,
                });
        }

        for peer in peer_map.values_mut() {
            if let Some(report) = self.load_node_report(&peer.node_id).await {
                peer.last_report_ms = Some(report.timestamp_ms);
                peer.last_seen_ms = Some(peer.last_seen_ms.map_or(report.timestamp_ms, |seen| seen.max(report.timestamp_ms)));
                peer.uptime_ms = Some(report.uptime_ms);
                peer.remote_pending_queue = Some(report.pending_queue);
                peer.remote_inbox_items = Some(report.inbox_items);
                peer.remote_wifi_mode = Some(report.wifi_mode.clone());
                peer.remote_wifi_ssid = report.wifi_ssid.clone();
                peer.remote_wifi_signal = report.wifi_signal;
                peer.power_state = report.power_state;
            }
            if let Some(policy) = self.peer_policies.get(&peer.node_id) {
                peer.role = policy.role.clone();
                peer.relationship = policy.relationship.clone();
                peer.preferred_transport_order = policy.preferred_transport_order.clone();
            }
            peer.alive = !peer.links.is_empty()
                || peer
                    .last_seen_ms
                    .is_some_and(|seen| now_ms().saturating_sub(seen) <= self.config.discovery.peer_ttl_ms);
            if peer.alive && peer.power_state == "unknown" {
                peer.power_state = "awake".to_string();
            }
            peer.last_rtt_ms = peer.links.iter().filter_map(|link| link.last_rtt_ms).min();
            peer.connection_quality = summarize_peer_quality(&peer.links, peer.remote_wifi_signal, peer.alive);
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
            .read_connection_loop(&mut reader, &peer_id, &transport, &tx)
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
        transport: &TransportKind,
        writer_tx: &LinkSender,
    ) -> Result<()> {
        loop {
            match read_frame(reader).await {
                Ok(Frame::Data(envelope)) => {
                    self.peers.touch_rx(peer_id, transport).await;
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
                    self.peers.touch_rx(peer_id, transport).await;
                    self.storage.mark_ack(ack.message_id, peer_id).await?;
                }
                Ok(Frame::Ping(ping)) => {
                    self.peers.touch_rx(peer_id, transport).await;
                    writer_tx.send(Frame::Pong(ping)).await.context("failed to send pong")?;
                }
                Ok(Frame::Pong(ping)) => {
                    self.peers.touch_rx(peer_id, transport).await;
                    self.peers.record_rtt(peer_id, transport, now_ms().saturating_sub(ping.timestamp_ms)).await;
                }
                Ok(Frame::Hello(_)) => {
                    self.peers.touch_rx(peer_id, transport).await;
                }
                Ok(Frame::Subscribe(sub)) => {
                    self.peers.touch_rx(peer_id, transport).await;
                    self.handle_subscribe(peer_id, sub).await;
                }
                Ok(Frame::Unsubscribe(unsub)) => {
                    self.peers.touch_rx(peer_id, transport).await;
                    self.handle_unsubscribe(peer_id, unsub).await;
                }
                Ok(Frame::StreamOpen(open)) => {
                    self.peers.touch_rx(peer_id, transport).await;
                    self.storage.stream_open(open).await;
                }
                Ok(Frame::StreamChunk(chunk)) => {
                    self.peers.touch_rx(peer_id, transport).await;
                    if let Some(assembled) = self.storage.stream_chunk(chunk).await {
                        self.storage.stream_close(assembled).await;
                    }
                }
                Ok(Frame::StreamClose(close)) => {
                    self.peers.touch_rx(peer_id, transport).await;
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

    async fn wifi_auto_manage_loop(self: Arc<Self>) {
        if let Err(error) = wifi::apply_mode(&self.config.wifi, None).await {
            warn!(%error, "initial Wi-Fi automation failed");
        }
        let interval = Duration::from_millis(self.config.wifi.status_interval_ms.max(1_000));
        loop {
            let status = wifi::collect_status(&self.config.wifi).await;
            let should_reapply = match self.config.wifi.desired_mode.as_str() {
                "parent" | "ap" | "hotspot" => status.mode != "ap",
                "child" | "client" => wifi::needs_client_reconnect(&self.config.wifi, &status),
                _ => false,
            };
            if should_reapply {
                if let Err(error) = wifi::apply_mode(&self.config.wifi, None).await {
                    warn!(%error, "Wi-Fi automation retry failed");
                }
            }
            sleep(interval).await;
        }
    }

    async fn node_status_report_loop(self: Arc<Self>) {
        let interval = Duration::from_millis(self.config.wifi.status_interval_ms.max(1_000));
        loop {
            if let Err(error) = self.publish_node_status().await {
                warn!(%error, "failed to publish node status");
            }
            sleep(interval).await;
        }
    }

    async fn publish_node_status(&self) -> Result<()> {
        let (pending_queue, inbox_items) = self.storage.counts().await;
        let wifi_status = wifi::collect_status(&self.config.wifi).await;
        let report = NodeRuntimeReport {
            node_id: self.node_id().to_string(),
            role: self.config.node.role.clone(),
            uptime_ms: now_ms().saturating_sub(self.started_at_ms),
            pending_queue,
            inbox_items,
            wifi_mode: wifi_status.mode,
            wifi_ssid: wifi_status.active_ssid,
            wifi_signal: wifi_status
                .scanned_networks
                .iter()
                .find(|network| network.active)
                .and_then(|network| network.signal),
            power_state: "awake".to_string(),
            timestamp_ms: now_ms(),
        };
        let payload = serde_json::to_vec(&report).context("failed to serialize node runtime report")?;
        let _ = self
            .submit_message(
                DeliveryTarget::Broadcast,
                TrafficClass::Telemetry,
                node_status_topic(self.node_id()),
                "node_status".to_string(),
                "application/json".to_string(),
                payload,
                Some(1),
            )
            .await?;
        Ok(())
    }

    async fn network_connector_loop(self: Arc<Self>, peer_id: String) {
        let interval = Duration::from_millis(self.config.transport.connect_interval_ms);
        loop {
            if self.peers.has_link(&peer_id, &TransportKind::Wifi).await {
                sleep(interval).await;
                continue;
            }

            let Some(addr) = self.resolve_network_addr(&peer_id).await else {
                sleep(interval).await;
                continue;
            };

            match TcpStream::connect(&addr).await {
                Ok(stream) => {
                    info!(peer_id = %peer_id, %addr, transport = "network", "outbound network transport connected");
                    if let Err(error) = self.clone().run_connection(stream, TransportKind::Wifi, Some(peer_id.clone())).await {
                        warn!(%error, peer_id = %peer_id, %addr, transport = "network", "outbound network connection ended");
                    }
                }
                Err(error) => {
                    debug!(%error, peer_id = %peer_id, %addr, transport = "network", "network connect attempt failed");
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
                        !message.acked_peers.contains(&peer_id)
                    };

                    if !should_retry {
                        continue;
                    }

                    let transport_order = self
                        .peer_policies
                        .get(&peer_id)
                        .map(|policy| policy.preferred_transport_order.clone())
                        .unwrap_or_else(default_transport_order);
                    if let Some(sender) = self.peers.best_sender(&peer_id, &transport_order).await {
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
            let cutoff = now_ms().saturating_sub(self.config.discovery.peer_ttl_ms);
            self.discovered_peers.retain(|_, peer| peer.last_seen_ms >= cutoff);
            sleep(interval).await;
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

    pub fn storage(&self) -> Arc<Storage> {
        self.storage.clone()
    }

    pub async fn wifi_status(&self) -> wifi::WifiStatus {
        wifi::collect_status(&self.config.wifi).await
    }

    pub async fn apply_wifi_mode(&self, mode: Option<&str>) -> Result<wifi::WifiActionResponse> {
        wifi::apply_mode(&self.config.wifi, mode).await
    }

    pub async fn start_hotspot(&self) -> Result<wifi::WifiActionResponse> {
        wifi::start_hotspot_now(&self.config.wifi).await
    }

    pub async fn connect_wifi(&self, ssid: &str, password: Option<&str>) -> Result<wifi::WifiActionResponse> {
        let effective_password = password.or_else(|| wifi::fallback_password(&self.config.wifi.fallback_networks, ssid));
        wifi::connect_ssid_now(&self.config.wifi, ssid, effective_password).await
    }

    pub async fn disconnect_wifi(&self) -> Result<wifi::WifiActionResponse> {
        wifi::disconnect_now(&self.config.wifi).await
    }

    pub async fn list_peer_policies(&self) -> Vec<PeerPolicy> {
        let mut items = self
            .peer_policies
            .iter()
            .map(|entry| entry.value().clone())
            .collect::<Vec<_>>();
        items.sort_by(|left, right| left.node_id.cmp(&right.node_id));
        items
    }

    pub async fn update_peer_policy(&self, policy: PeerPolicy) -> Result<PeerPolicy> {
        let normalized = PeerPolicy {
            node_id: policy.node_id.clone(),
            role: policy.role.clone(),
            relationship: if policy.relationship.trim().is_empty() {
                "peer".to_string()
            } else {
                policy.relationship.trim().to_string()
            },
            preferred_transport_order: normalize_transport_order(&policy.preferred_transport_order),
        };
        self.peer_policies.insert(normalized.node_id.clone(), normalized.clone());
        Ok(normalized)
    }

    fn peer_policy_for(&self, peer_id: &str) -> PeerPolicy {
        self.peer_policies.get(peer_id).map(|entry| entry.value().clone()).unwrap_or(PeerPolicy {
            node_id: peer_id.to_string(),
            role: None,
            relationship: "peer".to_string(),
            preferred_transport_order: default_transport_order(),
        })
    }

    async fn load_node_report(&self, node_id: &str) -> Option<NodeRuntimeReport> {
        let topic = node_status_topic(node_id);
        let entry = self.storage.get_topic(&topic).await?;
        serde_json::from_slice(&entry.envelope.payload).ok()
    }

    fn spawn_network_connector_loop(self: &Arc<Self>, peer_id: String) {
        if self.network_connector_workers.insert(peer_id.clone(), ()).is_some() {
            return;
        }
        let runtime = self.clone();
        tokio::spawn(async move {
            runtime.network_connector_loop(peer_id).await;
        });
    }

    async fn resolve_network_addr(&self, peer_id: &str) -> Option<String> {
        let now = now_ms();
        if let Some(discovered) = self.discovered_peers.get(peer_id) {
            if now.saturating_sub(discovered.last_seen_ms) <= self.config.discovery.peer_ttl_ms {
                return Some(discovered.network_addr.clone());
            }
        }
        self.config
            .peers
            .iter()
            .find(|peer| peer.node_id == peer_id)
            .and_then(|peer| peer.network_addr.clone())
    }

    fn parse_port(addr: &str) -> Option<u16> {
        addr.parse::<SocketAddr>().ok().map(|socket| socket.port())
    }

    async fn discovery_listener_loop(self: Arc<Self>) -> Result<()> {
        let socket = UdpSocket::bind(&self.config.discovery.bind)
            .await
            .with_context(|| format!("failed to bind discovery socket on {}", self.config.discovery.bind))?;
        socket.set_broadcast(true).context("failed to enable UDP broadcast")?;
        info!(bind = %self.config.discovery.bind, announce = %self.config.discovery.announce_addr, "discovery listener started");

        let mut buffer = vec![0_u8; 4096];
        loop {
            let (size, source_addr) = socket.recv_from(&mut buffer).await.context("discovery recv failed")?;
            let packet: DiscoveryPacket = match serde_json::from_slice(&buffer[..size]) {
                Ok(packet) => packet,
                Err(error) => {
                    debug!(%error, %source_addr, "ignoring malformed discovery packet");
                    continue;
                }
            };
            if packet.node_id == self.node_id() {
                continue;
            }
            let Some(network_port) = packet.network_port else {
                continue;
            };
            let network_addr = match source_addr {
                SocketAddr::V4(addr) => SocketAddr::V4(SocketAddrV4::new(*addr.ip(), network_port)).to_string(),
                SocketAddr::V6(_) => continue,
            };
            let web_url = packet.web_port.map(|web_port| format!("http://{}:{}", source_addr.ip(), web_port));
            let node_id = packet.node_id;
            self.discovered_peers.insert(
                node_id.clone(),
                DiscoveredPeer {
                    network_addr,
                    web_url,
                    tags: packet.tags,
                    last_seen_ms: now_ms(),
                },
            );
            self.spawn_network_connector_loop(node_id);
        }
    }

    async fn discovery_announce_loop(self: Arc<Self>) -> Result<()> {
        let socket = UdpSocket::bind("0.0.0.0:0")
            .await
            .context("failed to bind discovery announce socket")?;
        socket.set_broadcast(true).context("failed to enable UDP broadcast")?;
        let interval = Duration::from_millis(self.config.discovery.announce_interval_ms);

        loop {
            let packet = DiscoveryPacket {
                version: 1,
                node_id: self.node_id().to_string(),
                tags: self.config.node.tags.clone(),
                network_port: self
                    .config
                    .transport
                    .network_listen
                    .as_deref()
                    .and_then(Self::parse_port),
                web_port: Self::parse_port(&self.config.web.bind),
                timestamp_ms: now_ms(),
            };
            let payload = serde_json::to_vec(&packet).context("failed to serialize discovery packet")?;
            if let Err(error) = socket.send_to(&payload, &self.config.discovery.announce_addr).await {
                warn!(%error, addr = %self.config.discovery.announce_addr, "failed to send discovery announce");
            }
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
            last_rx_ms: Some(now_ms()),
            last_rtt_ms: None,
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

    async fn best_sender(&self, peer_id: &str, transport_order: &[String]) -> Option<LinkSender> {
        let inner = self.inner.read().await;
        let entry = inner.get(peer_id)?;
        for transport in transport_order {
            match transport.as_str() {
                "usb" => {
                    if let Some(link) = &entry.usb {
                        return Some(link.sender.clone());
                    }
                }
                "network" | "wifi" => {
                    if let Some(link) = &entry.wifi {
                        return Some(link.sender.clone());
                    }
                }
                _ => {}
            }
        }
        None
    }

    async fn touch_rx(&self, peer_id: &str, transport: &TransportKind) {
        let mut inner = self.inner.write().await;
        if let Some(entry) = inner.get_mut(peer_id) {
            let last_rx_ms = Some(now_ms());
            match transport {
                TransportKind::Usb => {
                    if let Some(link) = entry.usb.as_mut() {
                        link.last_rx_ms = last_rx_ms;
                    }
                }
                TransportKind::Wifi => {
                    if let Some(link) = entry.wifi.as_mut() {
                        link.last_rx_ms = last_rx_ms;
                    }
                }
            }
        }
    }

    async fn record_rtt(&self, peer_id: &str, transport: &TransportKind, rtt_ms: u64) {
        let mut inner = self.inner.write().await;
        if let Some(entry) = inner.get_mut(peer_id) {
            match transport {
                TransportKind::Usb => {
                    if let Some(link) = entry.usb.as_mut() {
                        link.last_rtt_ms = Some(rtt_ms);
                    }
                }
                TransportKind::Wifi => {
                    if let Some(link) = entry.wifi.as_mut() {
                        link.last_rtt_ms = Some(rtt_ms);
                    }
                }
            }
        }
    }

    async fn connected_peer_ids(&self) -> Vec<String> {
        let inner = self.inner.read().await;
        inner.keys().cloned().collect()
    }

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
                        last_rx_ms: link.last_rx_ms,
                        last_rtt_ms: link.last_rtt_ms,
                        quality: classify_link_quality("usb", link.last_rtt_ms, None),
                    });
                }
                if let Some(link) = &links.wifi {
                    statuses.push(PeerLinkStatus {
                        transport: "network".to_string(),
                        remote_addr: link.remote_addr.clone(),
                        connected_at_ms: link.connected_at_ms,
                        last_rx_ms: link.last_rx_ms,
                        last_rtt_ms: link.last_rtt_ms,
                        quality: classify_link_quality("network", link.last_rtt_ms, None),
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

fn default_transport_order() -> Vec<String> {
    vec!["usb".to_string(), "network".to_string()]
}

fn normalize_transport_order(items: &[String]) -> Vec<String> {
    let mut normalized = items
        .iter()
        .map(|item| item.trim().to_ascii_lowercase())
        .filter(|item| matches!(item.as_str(), "usb" | "network" | "wifi"))
        .map(|item| if item == "wifi" { "network".to_string() } else { item })
        .collect::<Vec<_>>();
    if !normalized.iter().any(|item| item == "usb") {
        normalized.push("usb".to_string());
    }
    if !normalized.iter().any(|item| item == "network") {
        normalized.push("network".to_string());
    }
    normalized
}

fn node_status_topic(node_id: &str) -> String {
    format!("sys/node_status/{node_id}")
}

fn summarize_peer_quality(links: &[PeerLinkStatus], wifi_signal: Option<u8>, alive: bool) -> String {
    if !alive {
        return "offline".to_string();
    }
    if let Some(best_rtt) = links.iter().filter_map(|link| link.last_rtt_ms).min() {
        if best_rtt <= 20 {
            return "excellent".to_string();
        }
        if best_rtt <= 80 {
            return "good".to_string();
        }
        if best_rtt <= 200 {
            return "fair".to_string();
        }
        return "poor".to_string();
    }
    if let Some(signal) = wifi_signal {
        if signal >= 75 {
            return "good".to_string();
        }
        if signal >= 45 {
            return "fair".to_string();
        }
        return "poor".to_string();
    }
    if !links.is_empty() {
        return "connected".to_string();
    }
    "discovered".to_string()
}

fn classify_link_quality(transport: &str, rtt_ms: Option<u64>, wifi_signal: Option<u8>) -> String {
    if transport == "usb" {
        match rtt_ms {
            Some(rtt) if rtt <= 10 => "excellent".to_string(),
            Some(rtt) if rtt <= 40 => "good".to_string(),
            Some(_) => "fair".to_string(),
            None => "connected".to_string(),
        }
    } else if let Some(signal) = wifi_signal {
        if signal >= 75 {
            "good".to_string()
        } else if signal >= 45 {
            "fair".to_string()
        } else {
            "poor".to_string()
        }
    } else {
        match rtt_ms {
            Some(rtt) if rtt <= 40 => "good".to_string(),
            Some(rtt) if rtt <= 120 => "fair".to_string(),
            Some(_) => "poor".to_string(),
            None => "connected".to_string(),
        }
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
