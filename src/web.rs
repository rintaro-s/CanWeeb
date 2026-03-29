use crate::mesh::Runtime;
use crate::protocol::{payload_preview, DeliveryTarget, TrafficClass};
use anyhow::{Context, Result};
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{Path, Query, State, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::process::Command;
use tracing::{info, warn};
use uuid::Uuid;

pub async fn serve(runtime: Arc<Runtime>) -> Result<()> {
    let app = Router::new()
        .route("/", get(index))
        .route("/api/status", get(status))
        .route("/api/inbox", get(inbox))
        .route("/api/inbox/:message_id", get(inbox_item))
        .route("/api/messages", post(send_message))
        .route("/api/topics", get(list_topics))
        .route("/api/topic", get(get_topic))
        .route("/api/streams", get(list_streams))
        .route("/api/streams/:stream_id", get(get_stream))
        .route("/api/wifi-direct/run", post(run_wifi_direct))
        .route("/ws/topics", get(ws_topics))
        .route("/ws/streams", get(ws_streams))
        .with_state(runtime.clone());

    let bind_addr = runtime.web_bind_addr()?;
    let listener = TcpListener::bind(bind_addr)
        .await
        .with_context(|| format!("failed to bind web ui on {bind_addr}"))?;
    info!(%bind_addr, "web ui started");
    axum::serve(listener, app).await.context("web ui failed")?;
    Ok(())
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn status(State(runtime): State<Arc<Runtime>>) -> Json<crate::mesh::RuntimeStatus> {
    Json(runtime.status_snapshot().await)
}

async fn inbox(State(runtime): State<Arc<Runtime>>) -> Json<Vec<InboxSummary>> {
    let items = runtime.storage().list_inbox().await;
    Json(
        items
            .into_iter()
            .map(|item| InboxSummary {
                message_id: item.envelope.message_id.to_string(),
                source_node: item.envelope.source_node,
                target: item.envelope.target.label(),
                traffic_class: item.envelope.traffic_class.label().to_string(),
                subject: item.envelope.subject,
                content_type: item.envelope.content_type.clone(),
                created_at_ms: item.envelope.created_at_ms,
                received_at_ms: item.received_at_ms,
                payload_size: item.envelope.payload.len(),
                preview: payload_preview(&item.envelope.content_type, &item.envelope.payload),
            })
            .collect(),
    )
}

async fn inbox_item(
    Path(message_id): Path<String>,
    State(runtime): State<Arc<Runtime>>,
) -> Result<Json<InboxDetail>, AppError> {
    let message_id = Uuid::parse_str(&message_id).context("invalid message id")?;
    let item = runtime
        .storage()
        .get_inbox(message_id)
        .await
        .ok_or_else(|| AppError::not_found("message not found"))?;

    Ok(Json(InboxDetail {
        message_id: item.envelope.message_id.to_string(),
        source_node: item.envelope.source_node,
        target: item.envelope.target.label(),
        traffic_class: item.envelope.traffic_class.label().to_string(),
        subject: item.envelope.subject,
        content_type: item.envelope.content_type.clone(),
        created_at_ms: item.envelope.created_at_ms,
        received_at_ms: item.received_at_ms,
        payload_size: item.envelope.payload.len(),
        preview: payload_preview(&item.envelope.content_type, &item.envelope.payload),
        payload_base64: STANDARD.encode(item.envelope.payload),
    }))
}

async fn list_topics(State(runtime): State<Arc<Runtime>>) -> Json<Vec<TopicSummary>> {
    let items = runtime.storage().list_topics().await;
    Json(
        items
            .into_iter()
            .map(|entry| TopicSummary {
                topic: entry.envelope.topic.clone(),
                source_node: entry.envelope.source_node.clone(),
                content_type: entry.envelope.content_type.clone(),
                payload_size: entry.envelope.payload.len(),
                received_at_ms: entry.received_at_ms,
                preview: payload_preview(&entry.envelope.content_type, &entry.envelope.payload),
            })
            .collect(),
    )
}

#[derive(Deserialize)]
struct TopicQuery {
    name: String,
}

/// GET /api/topic?name=image/front
/// topic 名に '/' を含む場合があるため path ではなく query パラメータにする
async fn get_topic(
    Query(q): Query<TopicQuery>,
    State(runtime): State<Arc<Runtime>>,
) -> Result<Json<TopicDetail>, AppError> {
    let entry = runtime
        .storage()
        .get_topic(&q.name)
        .await
        .ok_or_else(|| AppError::not_found("topic not found"))?;
    Ok(Json(TopicDetail {
        topic: entry.envelope.topic.clone(),
        source_node: entry.envelope.source_node.clone(),
        content_type: entry.envelope.content_type.clone(),
        payload_size: entry.envelope.payload.len(),
        received_at_ms: entry.received_at_ms,
        preview: payload_preview(&entry.envelope.content_type, &entry.envelope.payload),
        payload_base64: STANDARD.encode(&entry.envelope.payload),
    }))
}

/// GET /api/streams/:stream_id - 完成 stream の payload を base64 で取得
async fn get_stream(
    Path(stream_id): Path<String>,
    State(runtime): State<Arc<Runtime>>,
) -> Result<Json<StreamDetail>, AppError> {
    let sid = Uuid::parse_str(&stream_id).context("invalid stream_id")?;
    let streams = runtime.storage().list_streams().await;
    let s = streams
        .into_iter()
        .find(|s| s.meta.stream_id == sid)
        .ok_or_else(|| AppError::not_found("stream not found"))?;
    Ok(Json(StreamDetail {
        stream_id: s.meta.stream_id.to_string(),
        topic: s.meta.topic.clone(),
        source_node: s.meta.source_node.clone(),
        content_type: s.meta.content_type.clone(),
        total_bytes: s.meta.total_bytes,
        completed_at_ms: s.completed_at_ms,
        payload_base64: STANDARD.encode(&s.data),
    }))
}

async fn list_streams(State(runtime): State<Arc<Runtime>>) -> Json<Vec<StreamSummary>> {
    let items = runtime.storage().list_streams().await;
    Json(
        items
            .into_iter()
            .map(|s| StreamSummary {
                stream_id: s.meta.stream_id.to_string(),
                topic: s.meta.topic.clone(),
                source_node: s.meta.source_node.clone(),
                content_type: s.meta.content_type.clone(),
                total_bytes: s.meta.total_bytes,
                completed_at_ms: s.completed_at_ms,
            })
            .collect(),
    )
}

/// WebSocket endpoint: 全 topic の telemetry 更新を push する
async fn ws_topics(
    ws: WebSocketUpgrade,
    State(runtime): State<Arc<Runtime>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws_topics(socket, runtime))
}

async fn handle_ws_topics(mut socket: WebSocket, runtime: Arc<Runtime>) {
    let mut rx = runtime.storage().topic_tx.subscribe();
    loop {
        match rx.recv().await {
            Ok(entry) => {
                let msg = serde_json::json!({
                    "topic": entry.envelope.topic,
                    "source_node": entry.envelope.source_node,
                    "content_type": entry.envelope.content_type,
                    "received_at_ms": entry.received_at_ms,
                    "payload_size": entry.envelope.payload.len(),
                    "preview": payload_preview(&entry.envelope.content_type, &entry.envelope.payload),
                });
                if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
                    break;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                warn!(n, "ws_topics lagged");
            }
            Err(_) => break,
        }
    }
}

/// WebSocket endpoint: stream 完成通知を push
async fn ws_streams(
    ws: WebSocketUpgrade,
    State(runtime): State<Arc<Runtime>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws_streams(socket, runtime))
}

async fn handle_ws_streams(mut socket: WebSocket, runtime: Arc<Runtime>) {
    let mut rx = runtime.storage().stream_tx.subscribe();
    loop {
        match rx.recv().await {
            Ok(assembled) => {
                let msg = serde_json::json!({
                    "stream_id": assembled.meta.stream_id,
                    "topic": assembled.meta.topic,
                    "source_node": assembled.meta.source_node,
                    "content_type": assembled.meta.content_type,
                    "total_bytes": assembled.meta.total_bytes,
                    "completed_at_ms": assembled.completed_at_ms,
                    "payload_base64": STANDARD.encode(&assembled.data),
                });
                if socket.send(Message::Text(msg.to_string().into())).await.is_err() {
                    break;
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                warn!(n, "ws_streams lagged");
            }
            Err(_) => break,
        }
    }
}

async fn send_message(
    State(runtime): State<Arc<Runtime>>,
    Json(request): Json<SendMessageRequest>,
) -> Result<Json<SendMessageResponse>, AppError> {
    let has_binary_payload = request.payload_base64.is_some();
    let target = DeliveryTarget::parse(&request.target).context("invalid target")?;
    let payload = if let Some(payload_base64) = request.payload_base64 {
        STANDARD
            .decode(payload_base64)
            .context("payload_base64 is not valid base64")?
    } else {
        request.text.unwrap_or_default().into_bytes()
    };

    let content_type = request.content_type.unwrap_or_else(|| {
        if has_binary_payload {
            "application/octet-stream".to_string()
        } else {
            "text/plain; charset=utf-8".to_string()
        }
    });

    let topic = request.topic.unwrap_or_default();
    let subject = request.subject.unwrap_or_else(|| if topic.is_empty() { "untitled".to_string() } else { topic.clone() });
    let traffic_class = match request.traffic_class.as_deref() {
        Some(raw) => TrafficClass::parse(raw).context("invalid traffic_class")?,
        None => TrafficClass::Control,
    };
    let envelope = runtime
        .submit_message(target, traffic_class, topic, subject, content_type, payload, request.ttl)
        .await?;

    Ok(Json(SendMessageResponse {
        message_id: envelope.message_id.to_string(),
    }))
}

async fn run_wifi_direct(
    Json(request): Json<WifiDirectCommandRequest>,
) -> Result<Json<WifiDirectCommandResponse>, AppError> {
    let output = Command::new("wpa_cli")
        .arg("-i")
        .arg(&request.interface)
        .args(&request.args)
        .output()
        .await
        .context("failed to execute wpa_cli")?;

    Ok(Json(WifiDirectCommandResponse {
        exit_code: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
    }))
}

#[derive(Debug)]
struct AppError {
    status: StatusCode,
    message: String,
}

impl AppError {
    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }
}

impl From<anyhow::Error> for AppError {
    fn from(error: anyhow::Error) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: error.to_string(),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        (self.status, Json(ErrorResponse { error: self.message })).into_response()
    }
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Deserialize)]
struct SendMessageRequest {
    target: String,
    traffic_class: Option<String>,
    topic: Option<String>,
    subject: Option<String>,
    content_type: Option<String>,
    text: Option<String>,
    payload_base64: Option<String>,
    ttl: Option<u8>,
}

#[derive(Serialize)]
struct TopicSummary {
    topic: String,
    source_node: String,
    content_type: String,
    payload_size: usize,
    received_at_ms: u64,
    preview: String,
}

#[derive(Serialize)]
struct TopicDetail {
    topic: String,
    source_node: String,
    content_type: String,
    payload_size: usize,
    received_at_ms: u64,
    preview: String,
    payload_base64: String,
}

#[derive(Serialize)]
struct StreamSummary {
    stream_id: String,
    topic: String,
    source_node: String,
    content_type: String,
    total_bytes: u64,
    completed_at_ms: u64,
}

#[derive(Serialize)]
struct StreamDetail {
    stream_id: String,
    topic: String,
    source_node: String,
    content_type: String,
    total_bytes: u64,
    completed_at_ms: u64,
    payload_base64: String,
}

#[derive(Serialize)]
struct SendMessageResponse {
    message_id: String,
}

#[derive(Serialize)]
struct InboxSummary {
    message_id: String,
    source_node: String,
    target: String,
    traffic_class: String,
    subject: String,
    content_type: String,
    created_at_ms: u64,
    received_at_ms: u64,
    payload_size: usize,
    preview: String,
}

#[derive(Serialize)]
struct InboxDetail {
    message_id: String,
    source_node: String,
    target: String,
    traffic_class: String,
    subject: String,
    content_type: String,
    created_at_ms: u64,
    received_at_ms: u64,
    payload_size: usize,
    preview: String,
    payload_base64: String,
}

#[derive(Deserialize)]
struct WifiDirectCommandRequest {
    interface: String,
    args: Vec<String>,
}

#[derive(Serialize)]
struct WifiDirectCommandResponse {
    exit_code: i32,
    stdout: String,
    stderr: String,
}

const INDEX_HTML: &str = r##"<!doctype html>
<html lang="ja">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>CANweeb</title>
  <style>
    *{box-sizing:border-box}
    body{font-family:system-ui,sans-serif;margin:0;padding:20px;background:#0f172a;color:#e2e8f0}
    h1{margin:0 0 16px;font-size:1.5rem;color:#f8fafc}
    h2{margin:0 0 12px;font-size:1rem;color:#cbd5e1}
    .grid{display:grid;gap:14px;grid-template-columns:repeat(auto-fit,minmax(320px,1fr))}
    .card{background:#111827;border:1px solid #1e293b;border-radius:10px;padding:14px}
    label{display:block;margin-top:8px;font-size:0.78rem;color:#64748b;font-weight:600;text-transform:uppercase;letter-spacing:.04em}
    input,textarea,select{width:100%;margin-top:3px;padding:7px 10px;border-radius:6px;border:1px solid #334155;background:#020617;color:#e2e8f0;font-size:0.85rem}
    button{width:100%;margin-top:10px;padding:8px;border-radius:6px;border:none;background:#2563eb;color:#fff;font-weight:700;cursor:pointer;font-size:0.85rem}
    button:hover{background:#3b82f6}
    button.danger{background:#dc2626}
    button.danger:hover{background:#ef4444}
    table{width:100%;border-collapse:collapse;font-size:0.8rem}
    td,th{border-bottom:1px solid #1e293b;padding:5px 7px;text-align:left;vertical-align:top}
    th{color:#475569;font-weight:600;font-size:0.75rem;text-transform:uppercase}
    pre,code{white-space:pre-wrap;word-break:break-all;font-size:0.78rem;font-family:ui-monospace,monospace}
    .mono{font-family:ui-monospace,monospace}
    .badge{display:inline-block;padding:1px 6px;border-radius:999px;font-size:0.7rem;font-weight:700}
    .ctrl{background:#1e3a5f;color:#60a5fa}
    .tele{background:#14532d;color:#4ade80}
    .strm{background:#3b1a5f;color:#c084fc}
    .logbox{max-height:180px;overflow-y:auto;margin-top:8px;border:1px solid #1e293b;border-radius:6px;padding:6px}
    .logrow{border-bottom:1px solid #1e293b;padding:3px 0;font-size:0.78rem}
    .tag{display:inline-block;padding:1px 5px;border-radius:4px;font-size:0.7rem;background:#1e293b;color:#94a3b8;margin-right:3px}
    .wsstate{display:inline-block;width:8px;height:8px;border-radius:50%;background:#64748b;margin-right:6px}
    .wsstate.on{background:#22c55e}
  </style>
</head>
<body>
<h1>CANweeb</h1>
<div class="grid">

  <section class="card">
    <h2>送信</h2>
    <label>Target</label>
    <input id="target" value="broadcast" placeholder="broadcast / node:node-b / nodes:a,b" />
    <label>Traffic Class</label>
    <select id="trafficClass">
      <option value="control">control — ACK あり・永続化あり</option>
      <option value="telemetry">telemetry — ACK なし・最新値キャッシュ</option>
      <option value="stream">stream — ACK なし・ring buffer</option>
    </select>
    <label>Topic <span style="font-weight:400;text-transform:none">(telemetry/stream 必須、例: imu / cmd/motor / image/front)</span></label>
    <input id="topic" value="" placeholder="imu" />
    <label>Subject <span style="font-weight:400;text-transform:none">(省略時 topic 名)</span></label>
    <input id="subject" value="" />
    <label>Content-Type</label>
    <input id="contentType" value="text/plain; charset=utf-8" />
    <label>Text payload</label>
    <textarea id="textPayload" rows="3">hello</textarea>
    <label>Binary file</label>
    <input id="filePayload" type="file" />
    <button onclick="sendMessage()">送信</button>
    <pre id="sendResult" style="margin-top:8px;color:#94a3b8"></pre>
  </section>

  <section class="card">
    <h2><span class="wsstate" id="topicWsDot"></span>Telemetry ライブ (WebSocket)</h2>
    <button onclick="toggleWs('topic')">接続</button>
    <div class="logbox" id="topicLog"></div>
  </section>

  <section class="card">
    <h2><span class="wsstate" id="streamWsDot"></span>Stream ライブ (WebSocket)</h2>
    <p style="font-size:0.78rem;color:#64748b;margin:0 0 6px">chunked stream の完成通知を受信します。</p>
    <button onclick="toggleWs('stream')">接続</button>
    <div class="logbox" id="streamLog"></div>
  </section>

  <section class="card">
    <h2>Wi-Fi Direct / wpa_cli</h2>
    <label>Interface</label>
    <input id="wifiInterface" value="wlan0" />
    <label>Arguments (space-separated)</label>
    <input id="wifiArgs" value="status" />
    <button onclick="runWifiCommand()">実行</button>
    <pre id="wifiResult" style="margin-top:8px;color:#94a3b8"></pre>
  </section>

</div>

<section class="card" style="margin-top:14px">
  <h2>Node Status</h2>
  <pre id="statusBox" style="color:#94a3b8"></pre>
</section>

<section class="card" style="margin-top:14px">
  <h2>Topics <span style="font-size:0.75rem;color:#475569;font-weight:400">(latest value per topic — 3 s 自動更新)</span></h2>
  <table>
    <thead><tr><th>Topic</th><th>From</th><th>Type</th><th>Size</th><th>Preview</th><th>Updated</th></tr></thead>
    <tbody id="topicsTable"></tbody>
  </table>
</section>

<section class="card" style="margin-top:14px">
  <h2>Streams <span style="font-size:0.75rem;color:#475569;font-weight:400">(ring buffer — 最新 8 件)</span></h2>
  <table>
    <thead><tr><th>Stream ID</th><th>Topic</th><th>From</th><th>Type</th><th>Bytes</th><th>Completed</th><th></th></tr></thead>
    <tbody id="streamsTable"></tbody>
  </table>
</section>

<section class="card" style="margin-top:14px">
  <h2>Inbox <span style="font-size:0.75rem;color:#475569;font-weight:400">(control class のみ保持)</span></h2>
  <table>
    <thead><tr><th>ID</th><th>From</th><th>Target</th><th>Class</th><th>Subject</th><th>Preview</th></tr></thead>
    <tbody id="inboxTable"></tbody>
  </table>
</section>

<script>
const ws = { topic: null, stream: null };

function esc(s) { return String(s).replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;'); }

function badge(cls) {
  const m = { control:'ctrl', telemetry:'tele', stream:'strm' };
  return `<span class="badge ${m[cls]||''}">${esc(cls)}</span>`;
}

function fmt(ms) { return new Date(ms).toLocaleTimeString(); }

async function refreshStatus() {
  try {
    const s = await fetch('/api/status').then(r => r.json());
    document.getElementById('statusBox').textContent = JSON.stringify(s, null, 2);
  } catch(e) { document.getElementById('statusBox').textContent = String(e); }
}

async function refreshTopics() {
  try {
    const items = await fetch('/api/topics').then(r => r.json());
    const tb = document.getElementById('topicsTable');
    tb.innerHTML = '';
    for (const t of items) {
      const tr = document.createElement('tr');
      tr.innerHTML = `<td class="mono">${esc(t.topic)}</td><td>${esc(t.source_node)}</td><td>${esc(t.content_type)}</td><td>${t.payload_size}</td><td><code>${esc(t.preview)}</code></td><td>${fmt(t.received_at_ms)}</td>`;
      tb.appendChild(tr);
    }
  } catch(e) {}
}

async function refreshStreams() {
  try {
    const items = await fetch('/api/streams').then(r => r.json());
    const tb = document.getElementById('streamsTable');
    tb.innerHTML = '';
    for (const s of items) {
      const tr = document.createElement('tr');
      tr.innerHTML = `<td class="mono">${esc(s.stream_id.slice(0,8))}</td><td class="mono">${esc(s.topic)}</td><td>${esc(s.source_node)}</td><td>${esc(s.content_type)}</td><td>${s.total_bytes}</td><td>${fmt(s.completed_at_ms)}</td><td><button style="width:auto;padding:3px 8px;margin:0" onclick="downloadStream('${esc(s.stream_id)}','${esc(s.content_type)}')">DL</button></td>`;
      tb.appendChild(tr);
    }
  } catch(e) {}
}

async function downloadStream(sid, ct) {
  try {
    const d = await fetch(`/api/streams/${sid}`).then(r => r.json());
    const bytes = Uint8Array.from(atob(d.payload_base64), c => c.charCodeAt(0));
    const blob = new Blob([bytes], { type: ct });
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = `stream-${sid.slice(0,8)}`;
    a.click();
  } catch(e) { alert(String(e)); }
}

async function refreshInbox() {
  try {
    const items = await fetch('/api/inbox').then(r => r.json());
    const tb = document.getElementById('inboxTable');
    tb.innerHTML = '';
    for (const item of items) {
      const tr = document.createElement('tr');
      tr.innerHTML = `<td class="mono"><a href="#" onclick="showInboxItem('${item.message_id}');return false">${item.message_id.slice(0,8)}</a></td><td>${esc(item.source_node)}</td><td>${esc(item.target)}</td><td>${badge(item.traffic_class)}</td><td>${esc(item.subject)}</td><td><code>${esc(item.preview)}</code></td>`;
      tb.appendChild(tr);
    }
  } catch(e) {}
}

async function showInboxItem(mid) {
  const item = await fetch(`/api/inbox/${mid}`).then(r => r.json());
  alert(JSON.stringify(item, null, 2));
}

async function sendMessage() {
  const fileInput = document.getElementById('filePayload');
  let payload_base64 = null;
  let contentType = document.getElementById('contentType').value;
  if (fileInput.files.length > 0) {
    const file = fileInput.files[0];
    const buf = await file.arrayBuffer();
    const bytes = new Uint8Array(buf);
    let b = '';
    for (const x of bytes) b += String.fromCharCode(x);
    payload_base64 = btoa(b);
    contentType = file.type || contentType || 'application/octet-stream';
  }
  const topic = document.getElementById('topic').value.trim();
  const subject = document.getElementById('subject').value.trim() || topic || 'untitled';
  try {
    const res = await fetch('/api/messages', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        target: document.getElementById('target').value,
        traffic_class: document.getElementById('trafficClass').value,
        topic: topic || null,
        subject,
        content_type: contentType,
        text: payload_base64 ? null : document.getElementById('textPayload').value,
        payload_base64,
      }),
    });
    document.getElementById('sendResult').textContent = JSON.stringify(await res.json(), null, 2);
    refreshStatus();
  } catch(e) { document.getElementById('sendResult').textContent = String(e); }
}

function toggleWs(kind) {
  const key = kind;
  const dotId = kind + 'WsDot';
  const logId = kind + 'Log';
  const url = kind === 'topic' ? '/ws/topics' : '/ws/streams';
  if (ws[key] && ws[key].readyState < 2) {
    ws[key].close();
    ws[key] = null;
    return;
  }
  const proto = location.protocol === 'https:' ? 'wss' : 'ws';
  ws[key] = new WebSocket(`${proto}://${location.host}${url}`);
  ws[key].onopen = () => { document.getElementById(dotId).className = 'wsstate on'; };
  ws[key].onclose = () => { document.getElementById(dotId).className = 'wsstate'; };
  ws[key].onmessage = (ev) => {
    const d = JSON.parse(ev.data);
    const log = document.getElementById(logId);
    const row = document.createElement('div');
    row.className = 'logrow mono';
    if (kind === 'topic') {
      row.textContent = `[${fmt(d.received_at_ms)}] ${d.topic} (${d.source_node}) ${d.preview}`;
    } else {
      row.textContent = `[${fmt(d.completed_at_ms)}] ${d.topic} (${d.source_node}) ${d.total_bytes} B`;
    }
    log.prepend(row);
    if (kind === 'topic') refreshTopics();
    else refreshStreams();
  };
}

async function runWifiCommand() {
  try {
    const res = await fetch('/api/wifi-direct/run', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        interface: document.getElementById('wifiInterface').value,
        args: document.getElementById('wifiArgs').value.split(' ').filter(Boolean),
      }),
    });
    document.getElementById('wifiResult').textContent = JSON.stringify(await res.json(), null, 2);
  } catch(e) { document.getElementById('wifiResult').textContent = String(e); }
}

async function boot() {
  refreshStatus(); refreshTopics(); refreshStreams(); refreshInbox();
  setInterval(() => { refreshStatus(); refreshTopics(); refreshStreams(); refreshInbox(); }, 3000);
}
boot();
</script>
</body>
</html>
"##;
