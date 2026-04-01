use anyhow::{Context, Result};
use canweeb_cmdlib::prelude::*;
use clap::Parser;
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind},
    execute, queue,
    style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::io::{self, Stdout, Write};
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Interactive PWM TUI for Raspberry Pi GPIO 12/13/18/19"
)]
struct Args {
    #[arg(long, default_value_t = 18)]
    pin: u8,

    #[arg(long, default_value_t = 25_000)]
    frequency: u32,

    #[arg(long, default_value_t = 30.0)]
    duty: f64,

    #[arg(long, default_value_t = 5.0)]
    step: f64,

    #[arg(long, default_value_t = 1_000)]
    frequency_step: u32,

    #[arg(long, default_value_t = 0.0)]
    min_duty: f64,

    #[arg(long, default_value_t = 100.0)]
    max_duty: f64,

    #[arg(long)]
    sim: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputTarget {
    Duty,
    Frequency,
    Pin,
}

struct App {
    backend_label: &'static str,
    pwm: PwmOutput,
    step: f64,
    frequency_step: u32,
    min_duty: f64,
    max_duty: f64,
    input_target: InputTarget,
    input_buffer: String,
    status: String,
}

impl App {
    fn new(args: &Args, backend_label: &'static str) -> Self {
        let pwm = PwmOutput::new(args.pin)
            .frequency(args.frequency.max(1))
            .range(args.min_duty, args.max_duty)
            .duty_percent(args.duty);

        Self {
            backend_label,
            pwm,
            step: args.step,
            frequency_step: args.frequency_step,
            min_duty: args.min_duty,
            max_duty: args.max_duty,
            input_target: InputTarget::Duty,
            input_buffer: String::new(),
            status: String::from("ready"),
        }
    }

    fn apply_current_settings(&mut self) -> Result<()> {
        self.pwm.apply()
    }

    fn set_pin(&mut self, pin: u8) -> Result<()> {
        self.pwm = PwmOutput::new(pin)
            .frequency(self.pwm.frequency_hz())
            .range(self.min_duty, self.max_duty)
            .duty_percent(self.pwm.duty_percent());
        self.apply_current_settings()
    }

    fn set_frequency(&mut self, frequency_hz: u32) -> Result<()> {
        self.pwm.set_frequency(frequency_hz)
    }

    fn set_duty(&mut self, duty_percent: f64) -> Result<()> {
        self.pwm.set_duty_percent(duty_percent)
    }

    fn adjust_duty(&mut self, delta: f64) -> Result<()> {
        self.pwm.step_duty(delta)
    }

    fn adjust_frequency(&mut self, delta: i64) -> Result<()> {
        self.pwm.step_frequency(delta as i32)
    }

    fn commit_input(&mut self) -> Result<()> {
        let raw = self.input_buffer.trim();
        if raw.is_empty() {
            self.status = String::from("input cleared");
            self.input_buffer.clear();
            return Ok(());
        }

        let result = match self.input_target {
            InputTarget::Duty => raw
                .parse::<f64>()
                .map_err(|_| anyhow::anyhow!("invalid duty value"))
                .and_then(|value| self.set_duty(value).map_err(Into::into)),
            InputTarget::Frequency => raw
                .parse::<u32>()
                .map_err(|_| anyhow::anyhow!("invalid frequency value"))
                .and_then(|value| self.set_frequency(value).map_err(Into::into)),
            InputTarget::Pin => raw
                .parse::<u8>()
                .map_err(|_| anyhow::anyhow!("invalid pin value"))
                .and_then(|value| self.set_pin(value).map_err(Into::into)),
        };

        self.input_buffer.clear();
        match result {
            Ok(()) => {
                self.status = format!("updated {}", self.current_target_name());
                Ok(())
            }
            Err(err) => {
                self.status = err.to_string();
                Err(err)
            }
        }
    }

    fn current_target_name(&self) -> &'static str {
        match self.input_target {
            InputTarget::Duty => "duty",
            InputTarget::Frequency => "frequency",
            InputTarget::Pin => "pin",
        }
    }

    fn target_hint(&self) -> &'static str {
        match self.input_target {
            InputTarget::Duty => "type duty percent and press Enter",
            InputTarget::Frequency => "type frequency in Hz and press Enter",
            InputTarget::Pin => "type GPIO number and press Enter",
        }
    }

    fn shutdown(&mut self) {
        let _ = self.pwm.stop();
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    if args.min_duty > args.max_duty {
        anyhow::bail!("--min-duty must be <= --max-duty");
    }

    if args.sim {
        use_sim_backend().context("failed to switch to sim backend")?;
    } else {
        use_real_backend().context("failed to initialize real backend")?;
    }

    let backend_label = if args.sim { "sim" } else { "real" };
    let mut app = App::new(&args, backend_label);
    app.apply_current_settings().context("failed to apply initial PWM settings")?;

    let mut stdout = io::stdout();
    terminal::enable_raw_mode().context("failed to enable raw mode")?;
    if let Err(err) = execute!(stdout, EnterAlternateScreen, Hide) {
        let _ = terminal::disable_raw_mode();
        return Err(err).context("failed to enter alternate screen");
    }

    let run_result = run_loop(&mut stdout, &mut app);
    app.shutdown();
    let restore_result = restore_terminal(&mut stdout);

    run_result.and(restore_result)
}

fn run_loop(stdout: &mut Stdout, app: &mut App) -> Result<()> {
    loop {
        render(stdout, app).context("failed to render UI")?;

        if event::poll(Duration::from_millis(100)).context("failed to poll terminal events")? {
            match event::read().context("failed to read terminal event")? {
                Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                    if handle_key(app, key)? {
                        break;
                    }
                }
                _ => {}
            }
        }
    }

    Ok(())
}

fn handle_key(app: &mut App, key: KeyEvent) -> Result<bool> {
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => return Ok(true),
        KeyCode::Char('+') | KeyCode::Right => {
            match app.adjust_duty(app.step) {
                Ok(()) => app.status = format!("duty -> {:.1}%", app.pwm.duty_percent()),
                Err(err) => app.status = err.to_string(),
            }
        }
        KeyCode::Char('-') | KeyCode::Left => {
            match app.adjust_duty(-app.step) {
                Ok(()) => app.status = format!("duty -> {:.1}%", app.pwm.duty_percent()),
                Err(err) => app.status = err.to_string(),
            }
        }
        KeyCode::Up => {
            match app.adjust_frequency(app.frequency_step as i64) {
                Ok(()) => app.status = format!("frequency -> {} Hz", app.pwm.frequency_hz()),
                Err(err) => app.status = err.to_string(),
            }
        }
        KeyCode::Down => {
            match app.adjust_frequency(-(app.frequency_step as i64)) {
                Ok(()) => app.status = format!("frequency -> {} Hz", app.pwm.frequency_hz()),
                Err(err) => app.status = err.to_string(),
            }
        }
        KeyCode::Char('d') => {
            app.input_target = InputTarget::Duty;
            app.input_buffer.clear();
            app.status = String::from("input target set to duty");
        }
        KeyCode::Char('f') => {
            app.input_target = InputTarget::Frequency;
            app.input_buffer.clear();
            app.status = String::from("input target set to frequency");
        }
        KeyCode::Char('p') => {
            app.input_target = InputTarget::Pin;
            app.input_buffer.clear();
            app.status = String::from("input target set to pin");
        }
        KeyCode::Char('c') => {
            app.input_buffer.clear();
            app.status = String::from("input cleared");
        }
        KeyCode::Home => {
            match app.set_duty(app.min_duty) {
                Ok(()) => app.status = format!("duty -> {:.1}%", app.pwm.duty_percent()),
                Err(err) => app.status = err.to_string(),
            }
        }
        KeyCode::End => {
            match app.set_duty(app.max_duty) {
                Ok(()) => app.status = format!("duty -> {:.1}%", app.pwm.duty_percent()),
                Err(err) => app.status = err.to_string(),
            }
        }
        KeyCode::Backspace => {
            app.input_buffer.pop();
        }
        KeyCode::Enter => {
            let _ = app.commit_input();
        }
        KeyCode::Char(ch) if ch.is_ascii_digit() || ch == '.' => {
            app.input_buffer.push(ch);
        }
        _ => {}
    }

    Ok(false)
}

fn render(stdout: &mut Stdout, app: &App) -> io::Result<()> {
    let (cols, _) = terminal::size().unwrap_or((80, 24));
    let bar_width = usize::from(cols.saturating_sub(20)).max(10);
    let filled = ((app.pwm.duty_percent() / 100.0) * bar_width as f64)
        .round()
        .clamp(0.0, bar_width as f64) as usize;
    let bar = format!("[{}{}]", "#".repeat(filled), "-".repeat(bar_width - filled));
    let buffer_text = if app.input_buffer.is_empty() {
        String::from("<empty>")
    } else {
        app.input_buffer.clone()
    };

    queue!(stdout, MoveTo(0, 0), Clear(ClearType::All), SetForegroundColor(Color::Cyan))?;
    queue!(stdout, SetAttribute(Attribute::Bold), Print("CANweeb PWM TUI\n"), SetAttribute(Attribute::Reset), ResetColor)?;
    queue!(
        stdout,
        Print(format!(
            "Backend: {}  Pin: GPIO{}  Frequency: {} Hz  Duty: {:.1}%\n",
            app.backend_label,
            app.pwm.pin(),
            app.pwm.frequency_hz(),
            app.pwm.duty_percent()
        ))
    )?;
    queue!(stdout, Print(format!("Duty bar: {}\n\n", bar)))?;
    queue!(stdout, Print(format!("Input target: {}\n", app.current_target_name())))?;
    queue!(stdout, Print(format!("Buffer: {}\n", buffer_text)))?;
    queue!(stdout, Print(format!("Status: {}\n\n", app.status)))?;
    queue!(stdout, SetForegroundColor(Color::Yellow))?;
    queue!(stdout, Print("Keys: +/- or Left/Right duty, Up/Down frequency, d/f/p select target, digits + Enter apply, q quit\n"))?;
    queue!(stdout, Print(format!("Hint: {}\n", app.target_hint())))?;
    queue!(stdout, ResetColor)?;
    stdout.flush()
}

fn restore_terminal(stdout: &mut Stdout) -> Result<()> {
    execute!(stdout, Show, LeaveAlternateScreen).context("failed to leave alternate screen")?;
    terminal::disable_raw_mode().context("failed to disable raw mode")?;
    Ok(())
}