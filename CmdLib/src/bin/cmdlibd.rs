use clap::{Args, Parser, Subcommand};
use crossterm::cursor;
use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{self, ClearType};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Parser, Debug)]
#[command(name = "cmdlibd")]
#[command(about = "CmdLib program daemon and controller")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Daemon(DaemonOpts),
    Register(RegisterOpts),
    Unregister(UnregisterOpts),
    List(SocketOpt),
    Status(SocketOpt),
    Run(RunOpts),
    Stop(StopOpts),
    RunRaw(RunRawOpts),
    Tui(TuiOpts),
}

#[derive(Args, Debug)]
struct DaemonOpts {
    #[arg(long, default_value_t = default_socket_path())]
    socket: String,
    #[arg(long, default_value_t = default_registry_path())]
    registry: String,
    #[arg(long, default_value_t = 50)]
    tick_ms: u64,
}

#[derive(Args, Debug)]
struct SocketOpt {
    #[arg(long, default_value_t = default_socket_path())]
    socket: String,
}

#[derive(Args, Debug)]
struct RegisterOpts {
    #[arg(long, default_value_t = default_socket_path())]
    socket: String,
    #[arg(long)]
    name: String,
    #[arg(long)]
    command: String,
    #[arg(long = "arg")]
    args: Vec<String>,
    #[arg(long)]
    cwd: Option<String>,
    #[arg(long = "env")]
    env: Vec<String>,
    #[arg(long)]
    ttl_seconds: Option<u64>,
    #[arg(long, default_value_t = false)]
    allow_concurrent: bool,
}

#[derive(Args, Debug)]
struct UnregisterOpts {
    #[arg(long, default_value_t = default_socket_path())]
    socket: String,
    #[arg(long)]
    name: String,
}

#[derive(Args, Debug)]
struct RunOpts {
    #[arg(long, default_value_t = default_socket_path())]
    socket: String,
    #[arg(long)]
    name: String,
    #[arg(long)]
    ttl_seconds: Option<u64>,
}

#[derive(Args, Debug)]
struct StopOpts {
    #[arg(long, default_value_t = default_socket_path())]
    socket: String,
    #[arg(long)]
    name: String,
}

#[derive(Args, Debug)]
struct RunRawOpts {
    #[arg(long, default_value_t = default_socket_path())]
    socket: String,
    #[arg(long)]
    command: String,
    #[arg(long = "arg")]
    args: Vec<String>,
    #[arg(long)]
    cwd: Option<String>,
    #[arg(long = "env")]
    env: Vec<String>,
    #[arg(long)]
    ttl_seconds: Option<u64>,
}

#[derive(Args, Debug)]
struct TuiOpts {
    #[arg(long, default_value_t = default_socket_path())]
    socket: String,
    #[arg(long, default_value_t = 500)]
    refresh_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProgramSpec {
    name: String,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    cwd: Option<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    ttl_seconds: Option<u64>,
    #[serde(default)]
    allow_concurrent: bool,
}

#[derive(Debug)]
struct RunningProgram {
    run_id: String,
    name: String,
    command: String,
    args: Vec<String>,
    started_at: Instant,
    expires_at: Option<Instant>,
    child: Child,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RunningProgramView {
    run_id: String,
    name: String,
    pid: u32,
    command: String,
    args: Vec<String>,
    uptime_ms: u64,
    expires_in_ms: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum ApiRequest {
    Ping,
    Register { spec: ProgramSpec },
    Unregister { name: String },
    List,
    Status,
    Run { name: String, ttl_seconds: Option<u64> },
    Stop { name: String },
    RunRaw { spec: ProgramSpec },
}

#[derive(Debug, Serialize, Deserialize)]
struct ApiResponse {
    ok: bool,
    message: String,
    #[serde(default)]
    data: Value,
}

struct DaemonState {
    registry_path: PathBuf,
    registry: HashMap<String, ProgramSpec>,
    running: HashMap<String, RunningProgram>,
}

impl DaemonState {
    fn new(registry_path: impl Into<PathBuf>) -> Result<Self, String> {
        let registry_path = registry_path.into();
        let registry = load_registry(&registry_path)?;
        Ok(Self {
            registry_path,
            registry,
            running: HashMap::new(),
        })
    }

    fn save_registry(&self) -> Result<(), String> {
        save_registry(&self.registry_path, &self.registry)
    }

    fn tick(&mut self) {
        let mut finished = Vec::new();
        let mut expired = Vec::new();
        let now = Instant::now();

        for (run_id, proc_ref) in &mut self.running {
            if let Some(expires_at) = proc_ref.expires_at {
                if now >= expires_at {
                    let _ = proc_ref.child.kill();
                    let _ = proc_ref.child.wait();
                    expired.push(run_id.clone());
                    continue;
                }
            }

            match proc_ref.child.try_wait() {
                Ok(Some(_)) => finished.push(run_id.clone()),
                Ok(None) => {}
                Err(_) => finished.push(run_id.clone()),
            }
        }

        for run_id in finished {
            self.running.remove(&run_id);
        }
        for run_id in expired {
            self.running.remove(&run_id);
        }
    }

    fn to_running_views(&self) -> Vec<RunningProgramView> {
        let now = Instant::now();
        let mut out = Vec::new();
        for proc_ref in self.running.values() {
            let expires_in_ms = proc_ref
                .expires_at
                .and_then(|deadline| deadline.checked_duration_since(now))
                .map(|d| d.as_millis() as u64);
            out.push(RunningProgramView {
                run_id: proc_ref.run_id.clone(),
                name: proc_ref.name.clone(),
                pid: proc_ref.child.id(),
                command: proc_ref.command.clone(),
                args: proc_ref.args.clone(),
                uptime_ms: proc_ref.started_at.elapsed().as_millis() as u64,
                expires_in_ms,
            });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Daemon(opts) => run_daemon(opts),
        Commands::Register(opts) => {
            let spec = ProgramSpec {
                name: opts.name,
                command: opts.command,
                args: opts.args,
                cwd: opts.cwd,
                env: parse_env_pairs(&opts.env)?,
                ttl_seconds: opts.ttl_seconds,
                allow_concurrent: opts.allow_concurrent,
            };
            print_response(send_request(&opts.socket, &ApiRequest::Register { spec })?)
        }
        Commands::Unregister(opts) => print_response(send_request(
            &opts.socket,
            &ApiRequest::Unregister { name: opts.name },
        )?),
        Commands::List(opts) => print_response(send_request(&opts.socket, &ApiRequest::List)?),
        Commands::Status(opts) => {
            print_response(send_request(&opts.socket, &ApiRequest::Status)?)
        }
        Commands::Run(opts) => print_response(send_request(
            &opts.socket,
            &ApiRequest::Run {
                name: opts.name,
                ttl_seconds: opts.ttl_seconds,
            },
        )?),
        Commands::Stop(opts) => print_response(send_request(
            &opts.socket,
            &ApiRequest::Stop { name: opts.name },
        )?),
        Commands::RunRaw(opts) => {
            let spec = ProgramSpec {
                name: format!("raw-{}", Uuid::new_v4()),
                command: opts.command,
                args: opts.args,
                cwd: opts.cwd,
                env: parse_env_pairs(&opts.env)?,
                ttl_seconds: opts.ttl_seconds,
                allow_concurrent: true,
            };
            print_response(send_request(&opts.socket, &ApiRequest::RunRaw { spec })?)
        }
        Commands::Tui(opts) => run_tui(opts),
    }
}

fn run_daemon(opts: DaemonOpts) -> Result<(), String> {
    let socket_path = PathBuf::from(&opts.socket);
    if let Some(parent) = socket_path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create socket dir failed: {e}"))?;
    }
    if socket_path.exists() {
        fs::remove_file(&socket_path).map_err(|e| format!("remove stale socket failed: {e}"))?;
    }

    let listener = UnixListener::bind(&socket_path).map_err(|e| format!("bind socket failed: {e}"))?;
    listener
        .set_nonblocking(true)
        .map_err(|e| format!("set_nonblocking failed: {e}"))?;

    let mut state = DaemonState::new(opts.registry)?;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = handle_client(stream, &mut state);
            }
            Err(err) => {
                if err.kind() != std::io::ErrorKind::WouldBlock {
                    return Err(format!("accept failed: {err}"));
                }
            }
        }
        state.tick();
        thread::sleep(Duration::from_millis(opts.tick_ms));
    }
}

fn handle_client(stream: UnixStream, state: &mut DaemonState) -> Result<(), String> {
    let reader = BufReader::new(
        stream
            .try_clone()
            .map_err(|e| format!("clone stream failed: {e}"))?,
    );
    let mut lines = reader.lines();
    let line = match lines.next() {
        Some(Ok(line)) => line,
        Some(Err(err)) => return Err(format!("read request failed: {err}")),
        None => return Ok(()),
    };

    let req: ApiRequest =
        serde_json::from_str(&line).map_err(|e| format!("invalid request json: {e}"))?;
    let resp = process_request(state, req);

    let mut writer = BufWriter::new(stream);
    let encoded = serde_json::to_string(&resp).map_err(|e| format!("encode response failed: {e}"))?;
    writer
        .write_all(encoded.as_bytes())
        .map_err(|e| format!("write response failed: {e}"))?;
    writer
        .write_all(b"\n")
        .map_err(|e| format!("write response newline failed: {e}"))?;
    writer.flush().map_err(|e| format!("flush response failed: {e}"))?;
    Ok(())
}

fn process_request(state: &mut DaemonState, req: ApiRequest) -> ApiResponse {
    match req {
        ApiRequest::Ping => ok("pong", json!({ "ok": true })),
        ApiRequest::Register { spec } => {
            state.registry.insert(spec.name.clone(), spec.clone());
            match state.save_registry() {
                Ok(()) => ok("registered", json!({ "name": spec.name })),
                Err(save_err) => err(&format!("save registry failed: {save_err}")),
            }
        }
        ApiRequest::Unregister { name } => {
            let existed = state.registry.remove(&name).is_some();
            match state.save_registry() {
                Ok(()) => ok("unregistered", json!({ "name": name, "existed": existed })),
                Err(e) => err(&format!("save registry failed: {e}")),
            }
        }
        ApiRequest::List => {
            let mut items: Vec<ProgramSpec> = state.registry.values().cloned().collect();
            items.sort_by(|a, b| a.name.cmp(&b.name));
            ok("list", json!({ "programs": items }))
        }
        ApiRequest::Status => {
            let mut programs: Vec<ProgramSpec> = state.registry.values().cloned().collect();
            programs.sort_by(|a, b| a.name.cmp(&b.name));
            let running = state.to_running_views();
            ok("status", json!({ "programs": programs, "running": running }))
        }
        ApiRequest::Run { name, ttl_seconds } => {
            let Some(spec) = state.registry.get(&name).cloned() else {
                return err(&format!("program not found: {name}"));
            };
            run_spec(state, spec, ttl_seconds)
        }
        ApiRequest::Stop { name } => {
            let mut stopped = 0u64;
            let ids: Vec<String> = state
                .running
                .iter()
                .filter_map(|(id, proc_ref)| {
                    if proc_ref.name == name {
                        Some(id.clone())
                    } else {
                        None
                    }
                })
                .collect();
            for id in ids {
                if let Some(mut proc_ref) = state.running.remove(&id) {
                    let _ = proc_ref.child.kill();
                    let _ = proc_ref.child.wait();
                    stopped += 1;
                }
            }
            ok("stopped", json!({ "name": name, "stopped": stopped }))
        }
        ApiRequest::RunRaw { spec } => run_spec(state, spec, None),
    }
}

fn run_spec(state: &mut DaemonState, spec: ProgramSpec, ttl_override: Option<u64>) -> ApiResponse {
    if !spec.allow_concurrent
        && state
            .running
            .values()
            .any(|proc_ref| proc_ref.name == spec.name)
    {
        return err(&format!(
            "program already running and allow_concurrent=false: {}",
            spec.name
        ));
    }

    let mut command = Command::new(&spec.command);
    command.args(&spec.args);
    if let Some(cwd) = &spec.cwd {
        command.current_dir(cwd);
    }
    for (k, v) in &spec.env {
        command.env(k, v);
    }
    command.stdin(Stdio::null());
    command.stdout(Stdio::inherit());
    command.stderr(Stdio::inherit());

    match command.spawn() {
        Ok(child) => {
            let run_id = Uuid::new_v4().to_string();
            let ttl = ttl_override.or(spec.ttl_seconds);
            let started_at = Instant::now();
            let expires_at = ttl.map(|secs| started_at + Duration::from_secs(secs));
            let pid = child.id();
            state.running.insert(
                run_id.clone(),
                RunningProgram {
                    run_id: run_id.clone(),
                    name: spec.name.clone(),
                    command: spec.command.clone(),
                    args: spec.args.clone(),
                    started_at,
                    expires_at,
                    child,
                },
            );
            ok(
                "started",
                json!({
                    "run_id": run_id,
                    "name": spec.name,
                    "pid": pid,
                    "ttl_seconds": ttl,
                }),
            )
        }
        Err(err_spawn) => err(&format!("spawn failed: {err_spawn}")),
    }
}

fn send_request(socket: &str, req: &ApiRequest) -> Result<ApiResponse, String> {
    let stream = UnixStream::connect(socket).map_err(|e| format!("connect socket failed: {e}"))?;
    let mut writer = BufWriter::new(
        stream
            .try_clone()
            .map_err(|e| format!("clone stream failed: {e}"))?,
    );
    let encoded = serde_json::to_string(req).map_err(|e| format!("encode request failed: {e}"))?;
    writer
        .write_all(encoded.as_bytes())
        .map_err(|e| format!("write request failed: {e}"))?;
    writer
        .write_all(b"\n")
        .map_err(|e| format!("write request newline failed: {e}"))?;
    writer.flush().map_err(|e| format!("flush request failed: {e}"))?;

    let reader = BufReader::new(stream);
    let mut lines = reader.lines();
    let Some(line) = lines.next() else {
        return Err("empty response".to_string());
    };
    let line = line.map_err(|e| format!("read response failed: {e}"))?;
    serde_json::from_str(&line).map_err(|e| format!("decode response failed: {e}"))
}

fn run_tui(opts: TuiOpts) -> Result<(), String> {
    let mut stdout = std::io::stdout();
    terminal::enable_raw_mode().map_err(|e| format!("enable raw mode failed: {e}"))?;
    execute!(stdout, terminal::EnterAlternateScreen)
        .map_err(|e| format!("enter alt screen failed: {e}"))?;

    let mut selected = 0usize;
    let mut cached_programs: Vec<ProgramSpec> = Vec::new();
    let mut last_message = String::from("Up/Down: select, Enter: run, s: stop, r: refresh, q: quit");

    let refresh = Duration::from_millis(opts.refresh_ms);
    let mut last_refresh = Instant::now() - refresh;

    loop {
        if last_refresh.elapsed() >= refresh {
            match send_request(&opts.socket, &ApiRequest::Status) {
                Ok(resp) if resp.ok => {
                    let programs = resp
                        .data
                        .get("programs")
                        .cloned()
                        .unwrap_or_else(|| json!([]));
                    cached_programs = serde_json::from_value(programs).unwrap_or_default();
                    cached_programs.sort_by(|a, b| a.name.cmp(&b.name));
                    if selected >= cached_programs.len() && !cached_programs.is_empty() {
                        selected = cached_programs.len() - 1;
                    }
                }
                Ok(resp) => {
                    last_message = format!("status error: {}", resp.message);
                }
                Err(err_socket) => {
                    last_message = format!("socket error: {err_socket}");
                    cached_programs.clear();
                }
            }
            draw_tui(&opts.socket, &cached_programs, selected, &last_message)?;
            last_refresh = Instant::now();
        }

        if event::poll(Duration::from_millis(50)).map_err(|e| format!("poll key failed: {e}"))? {
            let evt = event::read().map_err(|e| format!("read key failed: {e}"))?;
            if let Event::Key(key) = evt {
                match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Down => {
                        if !cached_programs.is_empty() {
                            selected = (selected + 1).min(cached_programs.len() - 1);
                        }
                    }
                    KeyCode::Up => {
                        selected = selected.saturating_sub(1);
                    }
                    KeyCode::Enter => {
                        if let Some(program) = cached_programs.get(selected) {
                            match send_request(
                                &opts.socket,
                                &ApiRequest::Run {
                                    name: program.name.clone(),
                                    ttl_seconds: None,
                                },
                            ) {
                                Ok(resp) => {
                                    last_message = format!("run {}: {}", program.name, resp.message)
                                }
                                Err(err) => {
                                    last_message = format!("run {} failed: {err}", program.name)
                                }
                            }
                        }
                    }
                    KeyCode::Char('s') => {
                        if let Some(program) = cached_programs.get(selected) {
                            match send_request(
                                &opts.socket,
                                &ApiRequest::Stop {
                                    name: program.name.clone(),
                                },
                            ) {
                                Ok(resp) => {
                                    last_message = format!("stop {}: {}", program.name, resp.message)
                                }
                                Err(err) => {
                                    last_message = format!("stop {} failed: {err}", program.name)
                                }
                            }
                        }
                    }
                    KeyCode::Char('r') => {
                        last_refresh = Instant::now() - refresh;
                    }
                    _ => {}
                }
            }
        }
    }

    execute!(stdout, terminal::LeaveAlternateScreen)
        .map_err(|e| format!("leave alt screen failed: {e}"))?;
    terminal::disable_raw_mode().map_err(|e| format!("disable raw mode failed: {e}"))?;
    Ok(())
}

fn draw_tui(
    socket: &str,
    programs: &[ProgramSpec],
    selected: usize,
    last_message: &str,
) -> Result<(), String> {
    let status_resp = send_request(socket, &ApiRequest::Status).unwrap_or_else(|_| ApiResponse {
        ok: false,
        message: "status unavailable".to_string(),
        data: json!({}),
    });
    let running: Vec<RunningProgramView> = status_resp
        .data
        .get("running")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    let mut running_count: HashMap<String, usize> = HashMap::new();
    for row in running {
        *running_count.entry(row.name).or_insert(0) += 1;
    }

    let mut stdout = std::io::stdout();
    execute!(
        stdout,
        terminal::Clear(ClearType::All),
        cursor::MoveTo(0, 0)
    )
    .map_err(|e| format!("draw clear failed: {e}"))?;

    println!("CmdLib Daemon TUI");
    println!("status: {}", if status_resp.ok { "connected" } else { "disconnected" });
    println!();
    println!("registered programs:");

    if programs.is_empty() {
        println!("  (none)");
    } else {
        for (idx, program) in programs.iter().enumerate() {
            let marker = if idx == selected { ">" } else { " " };
            let runs = running_count.get(&program.name).copied().unwrap_or(0);
            println!(
                "{} {:<24} running={:<2} ttl={:<8} cmd={} {}",
                marker,
                program.name,
                runs,
                program
                    .ttl_seconds
                    .map(|v| format!("{}s", v))
                    .unwrap_or_else(|| "none".to_string()),
                program.command,
                program.args.join(" "),
            );
        }
    }

    println!();
    println!("last: {}", last_message);
    println!("keys: Up/Down select, Enter run, s stop, r refresh, q quit");
    Ok(())
}

fn print_response(resp: ApiResponse) -> Result<(), String> {
    if resp.ok {
        println!("{}", resp.message);
        if !resp.data.is_null() {
            println!(
                "{}",
                serde_json::to_string_pretty(&resp.data)
                    .map_err(|e| format!("format response failed: {e}"))?
            );
        }
        Ok(())
    } else {
        Err(resp.message)
    }
}

fn load_registry(path: &Path) -> Result<HashMap<String, ProgramSpec>, String> {
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let text = fs::read_to_string(path).map_err(|e| format!("read registry failed: {e}"))?;
    if text.trim().is_empty() {
        return Ok(HashMap::new());
    }
    serde_json::from_str(&text).map_err(|e| format!("parse registry failed: {e}"))
}

fn save_registry(path: &Path, map: &HashMap<String, ProgramSpec>) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create registry dir failed: {e}"))?;
    }
    let text = serde_json::to_string_pretty(map)
        .map_err(|e| format!("serialize registry failed: {e}"))?;
    fs::write(path, text).map_err(|e| format!("write registry failed: {e}"))
}

fn parse_env_pairs(items: &[String]) -> Result<HashMap<String, String>, String> {
    let mut out = HashMap::new();
    for item in items {
        let Some((key, value)) = item.split_once('=') else {
            return Err(format!("invalid --env format: {item} (expected KEY=VALUE)"));
        };
        out.insert(key.to_string(), value.to_string());
    }
    Ok(out)
}

fn default_socket_path() -> String {
    "/tmp/cmdlibd.sock".to_string()
}

fn default_registry_path() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    format!("{home}/.local/share/cmdlibd/registry.json")
}

fn ok(message: &str, data: Value) -> ApiResponse {
    ApiResponse {
        ok: true,
        message: message.to_string(),
        data,
    }
}

fn err(message: &str) -> ApiResponse {
    ApiResponse {
        ok: false,
        message: message.to_string(),
        data: Value::Null,
    }
}