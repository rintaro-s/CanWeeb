use crate::mesh::Runtime;
use crate::protocol::{payload_preview, DeliveryTarget, TrafficClass};
use anyhow::{Context, Result};
use axum::extract::{Path, State};
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
use tracing::info;
use uuid::Uuid;

pub async fn serve(runtime: Arc<Runtime>) -> Result<()> {
    let app = Router::new()
        .route("/", get(index))
        .route("/api/status", get(status))
        .route("/api/inbox", get(inbox))
        .route("/api/inbox/:message_id", get(inbox_item))
        .route("/api/messages", post(send_message))
        .route("/api/wifi-direct/run", post(run_wifi_direct))
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

    let subject = request.subject.unwrap_or_else(|| "untitled".to_string());
    let traffic_class = match request.traffic_class.as_deref() {
        Some(raw) => TrafficClass::parse(raw).context("invalid traffic_class")?,
        None => TrafficClass::Control,
    };
    let envelope = runtime
        .submit_message(target, traffic_class, subject, content_type, payload, request.ttl)
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
    subject: Option<String>,
    content_type: Option<String>,
    text: Option<String>,
    payload_base64: Option<String>,
    ttl: Option<u8>,
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
    body { font-family: system-ui, sans-serif; margin: 24px; background: #0f172a; color: #e2e8f0; }
    h1, h2 { margin-bottom: 8px; }
    .grid { display: grid; gap: 16px; grid-template-columns: repeat(auto-fit, minmax(320px, 1fr)); }
    .card { background: #111827; border: 1px solid #334155; border-radius: 12px; padding: 16px; }
    input, textarea, button { width: 100%; margin-top: 8px; padding: 10px; border-radius: 8px; border: 1px solid #475569; background: #020617; color: #e2e8f0; }
    button { background: #1d4ed8; cursor: pointer; }
    table { width: 100%; border-collapse: collapse; }
    td, th { border-bottom: 1px solid #334155; padding: 8px; text-align: left; vertical-align: top; }
    code, pre { white-space: pre-wrap; word-break: break-word; }
    .mono { font-family: ui-monospace, monospace; }
  </style>
</head>
<body>
  <h1>CANweeb</h1>
  <div class="grid">
    <section class="card">
      <h2>送信テスト</h2>
      <label>Target</label>
      <input id="target" value="broadcast" />
      <label>Traffic Class</label>
      <input id="trafficClass" value="control" />
      <label>Subject</label>
      <input id="subject" value="test-message" />
      <label>Content-Type</label>
      <input id="contentType" value="text/plain; charset=utf-8" />
      <label>Text</label>
      <textarea id="textPayload" rows="6">hello from web ui</textarea>
      <label>Binary file</label>
      <input id="filePayload" type="file" />
      <button onclick="sendMessage()">送信</button>
      <pre id="sendResult" class="mono"></pre>
      <p>Target 形式: <span class="mono">broadcast</span>, <span class="mono">node:node-b</span>, <span class="mono">nodes:node-b,node-c1</span></p>
    </section>

    <section class="card">
      <h2>Wi-Fi Direct / wpa_cli</h2>
      <label>Interface</label>
      <input id="wifiInterface" value="wlan0" />
      <label>Arguments (space separated)</label>
      <input id="wifiArgs" value="status" />
      <button onclick="runWifiCommand()">実行</button>
      <pre id="wifiResult" class="mono"></pre>
    </section>
  </div>

  <section class="card" style="margin-top:16px;">
    <h2>Status</h2>
    <pre id="statusBox" class="mono"></pre>
  </section>

  <section class="card" style="margin-top:16px;">
    <h2>Inbox</h2>
    <table>
      <thead>
        <tr>
          <th>Message</th>
          <th>From</th>
          <th>Target</th>
          <th>Class</th>
          <th>Subject</th>
          <th>Preview</th>
        </tr>
      </thead>
      <tbody id="inboxTable"></tbody>
    </table>
  </section>

<script>
async function refreshStatus() {
  const status = await fetch('/api/status').then(r => r.json());
  document.getElementById('statusBox').textContent = JSON.stringify(status, null, 2);
}

async function refreshInbox() {
  const items = await fetch('/api/inbox').then(r => r.json());
  const table = document.getElementById('inboxTable');
  table.innerHTML = '';
  for (const item of items) {
    const tr = document.createElement('tr');
    tr.innerHTML = `
      <td class="mono"><a href="#" onclick="showInboxItem('${item.message_id}')">${item.message_id}</a></td>
      <td>${item.source_node}</td>
      <td>${item.target}</td>
      <td>${item.traffic_class}</td>
      <td>${item.subject}</td>
      <td><code>${item.preview}</code></td>
    `;
    table.appendChild(tr);
  }
}

async function showInboxItem(messageId) {
  const item = await fetch(`/api/inbox/${messageId}`).then(r => r.json());
  alert(JSON.stringify(item, null, 2));
}

async function sendMessage() {
  const fileInput = document.getElementById('filePayload');
  let payload_base64 = null;
  let contentType = document.getElementById('contentType').value;
  if (fileInput.files.length > 0) {
    const file = fileInput.files[0];
    const buffer = await file.arrayBuffer();
    const bytes = new Uint8Array(buffer);
    let binary = '';
    for (const b of bytes) {
      binary += String.fromCharCode(b);
    }
    payload_base64 = btoa(binary);
    contentType = file.type || contentType || 'application/octet-stream';
  }

  const response = await fetch('/api/messages', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      target: document.getElementById('target').value,
      traffic_class: document.getElementById('trafficClass').value,
      subject: document.getElementById('subject').value,
      content_type: contentType,
      text: payload_base64 ? null : document.getElementById('textPayload').value,
      payload_base64,
    }),
  });
  const result = await response.json();
  document.getElementById('sendResult').textContent = JSON.stringify(result, null, 2);
  await refreshStatus();
}

async function runWifiCommand() {
  const response = await fetch('/api/wifi-direct/run', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      interface: document.getElementById('wifiInterface').value,
      args: document.getElementById('wifiArgs').value.split(' ').filter(Boolean),
    }),
  });
  const result = await response.json();
  document.getElementById('wifiResult').textContent = JSON.stringify(result, null, 2);
}

async function boot() {
  await refreshStatus();
  await refreshInbox();
  setInterval(async () => {
    await refreshStatus();
    await refreshInbox();
  }, 3000);
}
boot();
</script>
</body>
</html>
"##;
