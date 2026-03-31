#![allow(non_snake_case)]

use crate::types::PinMode;
use crate::{dispatch, CmdError};
use num_traits::{Float, Num, Signed};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde_json::json;
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use i2cdev::core::I2CDevice;
use i2cdev::linux::LinuxI2CDevice;
use serialport::SerialPort;
use spidev::{SpiModeFlags, Spidev, SpidevOptions, SpidevTransfer};

#[derive(Debug, Clone, Copy)]
pub enum BitOrder {
    LsbFirst,
    MsbFirst,
}

#[derive(Debug, Clone)]
pub enum AnalogReference {
    Default,
    Internal,
    External,
    Custom(String),
}

#[derive(Debug, Clone)]
pub struct WiFiNetwork {
    pub ssid: String,
    pub signal_dbm: Option<i32>,
    pub security: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct IPAddress(pub IpAddr);

pub trait Stream {
    fn read_bytes(&mut self, len: usize) -> Result<Vec<u8>, CmdError>;
    fn write_bytes(&mut self, data: &[u8]) -> Result<usize, CmdError>;
}

pub trait Print {
    fn print(&mut self, text: &str) -> Result<(), CmdError>;
    fn println(&mut self, text: &str) -> Result<(), CmdError> {
        self.print(text)?;
        self.print("\n")
    }
}

pub struct SPI {
    dev: Spidev,
}

impl SPI {
    pub fn open(path: &str, speed_hz: u32, mode: u8) -> Result<Self, CmdError> {
        let mut dev = Spidev::open(path)
            .map_err(|e| CmdError::Backend(format!("spi open failed: {e}")))?;
        let mode_flags = match mode {
            0 => SpiModeFlags::SPI_MODE_0,
            1 => SpiModeFlags::SPI_MODE_1,
            2 => SpiModeFlags::SPI_MODE_2,
            3 => SpiModeFlags::SPI_MODE_3,
            _ => {
                return Err(CmdError::InvalidArgument {
                    key: "mode".to_string(),
                    reason: "SPI mode must be 0..=3".to_string(),
                })
            }
        };

        let options = SpidevOptions::new()
            .bits_per_word(8)
            .max_speed_hz(speed_hz)
            .mode(mode_flags)
            .build();

        dev.configure(&options)
            .map_err(|e| CmdError::Backend(format!("spi configure failed: {e}")))?;

        Ok(Self { dev })
    }

    pub fn transfer(&mut self, tx: &[u8]) -> Result<Vec<u8>, CmdError> {
        let mut rx = vec![0u8; tx.len()];
        let mut tx_buf = tx.to_vec();
        let mut transfer = SpidevTransfer::read_write(&mut tx_buf, &mut rx);
        self.dev
            .transfer(&mut transfer)
            .map_err(|e| CmdError::Backend(format!("spi transfer failed: {e}")))?;
        Ok(rx)
    }
}

pub struct Serial {
    port: Box<dyn SerialPort>,
}

impl Serial {
    pub fn begin(path: &str, baud: u32) -> Result<Self, CmdError> {
        let port = serialport::new(path, baud)
            .timeout(Duration::from_millis(200))
            .open()
            .map_err(|e| CmdError::Backend(format!("serial open failed: {e}")))?;
        Ok(Self { port })
    }

    pub fn available(&mut self) -> Result<u32, CmdError> {
        self.port
            .bytes_to_read()
            .map_err(|e| CmdError::Backend(format!("serial available failed: {e}")))
    }

    pub fn read_line(&mut self) -> Result<String, CmdError> {
        let mut buf = vec![0u8; 256];
        let n = self
            .port
            .read(&mut buf)
            .map_err(|e| CmdError::Backend(format!("serial read failed: {e}")))?;
        Ok(String::from_utf8_lossy(&buf[..n]).to_string())
    }

    pub fn write_str(&mut self, data: &str) -> Result<(), CmdError> {
        self.port
            .write_all(data.as_bytes())
            .map_err(|e| CmdError::Backend(format!("serial write failed: {e}")))
    }
}

impl Stream for Serial {
    fn read_bytes(&mut self, len: usize) -> Result<Vec<u8>, CmdError> {
        let mut buf = vec![0u8; len];
        let n = self
            .port
            .read(&mut buf)
            .map_err(|e| CmdError::Backend(format!("serial read bytes failed: {e}")))?;
        buf.truncate(n);
        Ok(buf)
    }

    fn write_bytes(&mut self, data: &[u8]) -> Result<usize, CmdError> {
        self.port
            .write(data)
            .map_err(|e| CmdError::Backend(format!("serial write bytes failed: {e}")))
    }
}

impl Print for Serial {
    fn print(&mut self, text: &str) -> Result<(), CmdError> {
        self.write_str(text)
    }
}

pub struct Wire {
    dev: LinuxI2CDevice,
}

impl Wire {
    pub fn begin(bus: &str, address: u16) -> Result<Self, CmdError> {
        let dev = LinuxI2CDevice::new(bus, address)
            .map_err(|e| CmdError::Backend(format!("i2c open failed: {e}")))?;
        Ok(Self { dev })
    }

    pub fn write(&mut self, data: &[u8]) -> Result<(), CmdError> {
        self.dev
            .write(data)
            .map_err(|e| CmdError::Backend(format!("i2c write failed: {e}")))
    }

    pub fn read(&mut self, len: usize) -> Result<Vec<u8>, CmdError> {
        let mut out = vec![0u8; len];
        self.dev
            .read(&mut out)
            .map_err(|e| CmdError::Backend(format!("i2c read failed: {e}")))?;
        Ok(out)
    }
}

pub struct WiFiClient {
    stream: TcpStream,
}

impl WiFiClient {
    pub fn connect(addr: &str) -> Result<Self, CmdError> {
        let stream = TcpStream::connect(addr)
            .map_err(|e| CmdError::Backend(format!("wifi client connect failed: {e}")))?;
        Ok(Self { stream })
    }

    pub fn peer_addr(&self) -> Result<SocketAddr, CmdError> {
        self.stream
            .peer_addr()
            .map_err(|e| CmdError::Backend(format!("peer addr failed: {e}")))
    }
}

impl Stream for WiFiClient {
    fn read_bytes(&mut self, len: usize) -> Result<Vec<u8>, CmdError> {
        let mut buf = vec![0u8; len];
        let n = self
            .stream
            .read(&mut buf)
            .map_err(|e| CmdError::Backend(format!("wifi client read failed: {e}")))?;
        buf.truncate(n);
        Ok(buf)
    }

    fn write_bytes(&mut self, data: &[u8]) -> Result<usize, CmdError> {
        self.stream
            .write(data)
            .map_err(|e| CmdError::Backend(format!("wifi client write failed: {e}")))
    }
}

impl Print for WiFiClient {
    fn print(&mut self, text: &str) -> Result<(), CmdError> {
        self.stream
            .write_all(text.as_bytes())
            .map_err(|e| CmdError::Backend(format!("wifi client print failed: {e}")))
    }
}

pub struct WiFiServer {
    listener: TcpListener,
}

impl WiFiServer {
    pub fn bind(addr: &str) -> Result<Self, CmdError> {
        let listener = TcpListener::bind(addr)
            .map_err(|e| CmdError::Backend(format!("wifi server bind failed: {e}")))?;
        Ok(Self { listener })
    }

    pub fn local_addr(&self) -> Result<SocketAddr, CmdError> {
        self.listener
            .local_addr()
            .map_err(|e| CmdError::Backend(format!("wifi server local_addr failed: {e}")))
    }

    pub fn accept(&self) -> Result<(WiFiClient, SocketAddr), CmdError> {
        let (stream, addr) = self
            .listener
            .accept()
            .map_err(|e| CmdError::Backend(format!("wifi server accept failed: {e}")))?;
        Ok((WiFiClient { stream }, addr))
    }
}

pub struct WiFiUDP {
    socket: UdpSocket,
}

impl WiFiUDP {
    pub fn bind(addr: &str) -> Result<Self, CmdError> {
        let socket = UdpSocket::bind(addr)
            .map_err(|e| CmdError::Backend(format!("wifi udp bind failed: {e}")))?;
        Ok(Self { socket })
    }

    pub fn send_to(&self, data: &[u8], addr: &str) -> Result<usize, CmdError> {
        self.socket
            .send_to(data, addr)
            .map_err(|e| CmdError::Backend(format!("wifi udp send failed: {e}")))
    }

    pub fn local_addr(&self) -> Result<SocketAddr, CmdError> {
        self.socket
            .local_addr()
            .map_err(|e| CmdError::Backend(format!("wifi udp local_addr failed: {e}")))
    }

    pub fn recv_from(&self, max_len: usize) -> Result<(Vec<u8>, SocketAddr), CmdError> {
        let mut buf = vec![0u8; max_len];
        let (n, addr) = self
            .socket
            .recv_from(&mut buf)
            .map_err(|e| CmdError::Backend(format!("wifi udp recv failed: {e}")))?;
        buf.truncate(n);
        Ok((buf, addr))
    }
}

pub struct USB;

impl USB {
    pub fn list_devices() -> Result<Vec<String>, CmdError> {
        let root = Path::new("/sys/bus/usb/devices");
        let mut out = Vec::new();
        if !root.exists() {
            return Ok(out);
        }
        for entry in fs::read_dir(root).map_err(io_error("usb read_dir failed"))? {
            let entry = entry.map_err(io_error("usb dir entry failed"))?;
            out.push(entry.file_name().to_string_lossy().to_string());
        }
        out.sort();
        Ok(out)
    }
}

pub struct Keyboard;

impl Keyboard {
    pub fn list_devices() -> Result<Vec<String>, CmdError> {
        list_input_devices_containing("kbd")
    }
}

pub struct Mouse;

impl Mouse {
    pub fn list_devices() -> Result<Vec<String>, CmdError> {
        list_input_devices_containing("mouse")
    }

    pub fn read_delta_once() -> Result<(i8, i8), CmdError> {
        let mut file = File::open("/dev/input/mice")
            .map_err(|e| CmdError::Backend(format!("open /dev/input/mice failed: {e}")))?;
        let mut packet = [0u8; 3];
        file.read_exact(&mut packet)
            .map_err(|e| CmdError::Backend(format!("read /dev/input/mice failed: {e}")))?;
        Ok((packet[1] as i8, packet[2] as i8))
    }
}

fn list_input_devices_containing(keyword: &str) -> Result<Vec<String>, CmdError> {
    let mut out = Vec::new();
    let base = Path::new("/dev/input/by-id");
    if !base.exists() {
        return Ok(out);
    }
    for entry in fs::read_dir(base).map_err(io_error("input read_dir failed"))? {
        let entry = entry.map_err(io_error("input entry failed"))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if name.to_ascii_lowercase().contains(keyword) {
            out.push(entry.path().display().to_string());
        }
    }
    out.sort();
    Ok(out)
}

fn io_error(prefix: &'static str) -> impl Fn(std::io::Error) -> CmdError {
    move |e| CmdError::Backend(format!("{prefix}: {e}"))
}

fn command_ok(domain: &str, action: &str, args: serde_json::Value) -> Result<serde_json::Value, CmdError> {
    let result = dispatch(domain, action, args)?;
    if result.success {
        Ok(result.data)
    } else {
        Err(CmdError::Backend(result.message))
    }
}

fn mode_label(mode: PinMode) -> &'static str {
    match mode {
        PinMode::Input => "input",
        PinMode::Output => "output",
        PinMode::Pwm => "pwm",
        PinMode::Analog => "analog",
        PinMode::Interrupt => "interrupt",
    }
}

#[derive(Clone)]
pub struct InterruptCallback(pub Arc<dyn Fn() + Send + Sync + 'static>);

static START_TIME: OnceLock<Instant> = OnceLock::new();
static INTERRUPTS_ENABLED: AtomicBool = AtomicBool::new(true);
static CALLBACKS: OnceLock<Mutex<HashMap<u8, InterruptCallback>>> = OnceLock::new();
static RNG: OnceLock<Mutex<StdRng>> = OnceLock::new();

fn start_time() -> Instant {
    *START_TIME.get_or_init(Instant::now)
}

fn callback_store() -> &'static Mutex<HashMap<u8, InterruptCallback>> {
    CALLBACKS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn rng_store() -> &'static Mutex<StdRng> {
    RNG.get_or_init(|| Mutex::new(StdRng::seed_from_u64(0xC0DE_CAFE_u64)))
}

pub fn pinMode(pin: &str, mode: PinMode) -> Result<(), CmdError> {
    command_ok("gpio", "pin_mode", json!({ "pin": pin, "mode": mode_label(mode) }))?;
    Ok(())
}

pub fn digitalWrite(pin: &str, value: bool) -> Result<(), CmdError> {
    let level = if value { "high" } else { "low" };
    command_ok("gpio", "digital_write", json!({ "pin": pin, "level": level }))?;
    Ok(())
}

pub fn digitalRead(pin: &str) -> Result<bool, CmdError> {
    let data = command_ok("gpio", "digital_read", json!({ "pin": pin }))?;
    Ok(data
        .get("is_high")
        .and_then(|v| v.as_bool())
        .unwrap_or(false))
}

pub fn abs<T>(x: T) -> T
where
    T: Signed,
{
    x.abs()
}

pub fn constrain<T>(x: T, low: T, high: T) -> T
where
    T: PartialOrd + Copy,
{
    if x < low {
        low
    } else if x > high {
        high
    } else {
        x
    }
}

pub fn map(value: i64, from_low: i64, from_high: i64, to_low: i64, to_high: i64) -> i64 {
    if from_high == from_low {
        return to_low;
    }
    (value - from_low) * (to_high - to_low) / (from_high - from_low) + to_low
}

pub fn max<T>(a: T, b: T) -> T
where
    T: Ord,
{
    std::cmp::max(a, b)
}

pub fn min<T>(a: T, b: T) -> T
where
    T: Ord,
{
    std::cmp::min(a, b)
}

pub fn pow<T>(base: T, exp: T) -> T
where
    T: Float,
{
    base.powf(exp)
}

pub fn sq<T>(x: T) -> T
where
    T: Num + Copy,
{
    x * x
}

pub fn sqrt<T>(x: T) -> T
where
    T: Float,
{
    x.sqrt()
}

pub fn bit(bit_index: u8) -> u32 {
    1_u32 << bit_index
}

pub fn bitClear(value: u32, bit_index: u8) -> u32 {
    value & !(1_u32 << bit_index)
}

pub fn bitRead(value: u32, bit_index: u8) -> bool {
    (value & (1_u32 << bit_index)) != 0
}

pub fn bitSet(value: u32, bit_index: u8) -> u32 {
    value | (1_u32 << bit_index)
}

pub fn bitWrite(value: u32, bit_index: u8, bit_value: bool) -> u32 {
    if bit_value {
        bitSet(value, bit_index)
    } else {
        bitClear(value, bit_index)
    }
}

pub fn highByte(value: u16) -> u8 {
    ((value >> 8) & 0xFF) as u8
}

pub fn lowByte(value: u16) -> u8 {
    (value & 0xFF) as u8
}

pub fn analogRead(pin: u8) -> Result<u16, CmdError> {
    let data = command_ok("analog", "analog_read", json!({ "pin": pin }))?;
    let v = data
        .get("value")
        .and_then(|x| x.as_u64())
        .ok_or_else(|| CmdError::Backend("analog_read missing value".to_string()))?;
    Ok(v as u16)
}

pub fn analogReadResolution(bits: u8) -> Result<(), CmdError> {
    command_ok("analog", "analog_read_resolution", json!({ "bits": bits }))?;
    Ok(())
}

pub fn analogReference(reference: AnalogReference) -> Result<(), CmdError> {
    let label = match reference {
        AnalogReference::Default => "default".to_string(),
        AnalogReference::Internal => "internal".to_string(),
        AnalogReference::External => "external".to_string(),
        AnalogReference::Custom(s) => s,
    };
    command_ok("analog", "analog_reference", json!({ "reference": label }))?;
    Ok(())
}

pub fn analogWrite(pin: u8, value: u16) -> Result<(), CmdError> {
    command_ok("analog", "analog_write", json!({ "pin": pin, "value": value }))?;
    Ok(())
}

pub fn analogWriteResolution(bits: u8) -> Result<(), CmdError> {
    command_ok("analog", "analog_write_resolution", json!({ "bits": bits }))?;
    Ok(())
}

pub fn cos(x: f64) -> f64 {
    x.cos()
}

pub fn sin(x: f64) -> f64 {
    x.sin()
}

pub fn tan(x: f64) -> f64 {
    x.tan()
}

pub fn attachInterrupt(interrupt: u8, callback: InterruptCallback) -> Result<(), CmdError> {
    let mut map = callback_store()
        .lock()
        .map_err(|_| CmdError::Backend("interrupt callback lock poisoned".to_string()))?;
    map.insert(interrupt, callback);
    Ok(())
}

pub fn detachInterrupt(interrupt: u8) -> Result<(), CmdError> {
    let mut map = callback_store()
        .lock()
        .map_err(|_| CmdError::Backend("interrupt callback lock poisoned".to_string()))?;
    map.remove(&interrupt);
    Ok(())
}

pub fn digitalPinToInterrupt(pin: u8) -> i32 {
    pin as i32
}

pub fn triggerInterrupt(interrupt: u8) -> Result<(), CmdError> {
    if !INTERRUPTS_ENABLED.load(Ordering::SeqCst) {
        return Ok(());
    }
    let cb = {
        let map = callback_store()
            .lock()
            .map_err(|_| CmdError::Backend("interrupt callback lock poisoned".to_string()))?;
        map.get(&interrupt).cloned()
    };
    if let Some(callback) = cb {
        (callback.0)();
    }
    Ok(())
}

pub fn noTone(pin: &str) -> Result<(), CmdError> {
    command_ok("tone", "no_tone", json!({ "pin": pin }))?;
    Ok(())
}

pub fn tone(pin: &str, frequency_hz: u32, duration_ms: Option<u64>) -> Result<(), CmdError> {
    command_ok(
        "tone",
        "tone",
        json!({
            "pin": pin,
            "frequency_hz": frequency_hz,
            "duration_ms": duration_ms,
        }),
    )?;
    if let Some(ms) = duration_ms {
        let pin_owned = pin.to_string();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(ms));
            let _ = command_ok("tone", "no_tone", json!({ "pin": pin_owned }));
        });
    }
    Ok(())
}

pub fn pulseIn(pin: &str, state: bool, timeout_us: u64) -> Result<u32, CmdError> {
    Ok(pulseInLong(pin, state, timeout_us)? as u32)
}

pub fn pulseInLong(pin: &str, state: bool, timeout_us: u64) -> Result<u64, CmdError> {
    let start = Instant::now();
    while digitalRead(pin)? != state {
        if start.elapsed().as_micros() as u64 >= timeout_us {
            return Ok(0);
        }
    }
    let pulse_start = Instant::now();
    while digitalRead(pin)? == state {
        if start.elapsed().as_micros() as u64 >= timeout_us {
            return Ok(0);
        }
    }
    Ok(pulse_start.elapsed().as_micros() as u64)
}

pub fn shiftIn(data_pin: &str, clock_pin: &str, order: BitOrder) -> Result<u8, CmdError> {
    let mut value = 0u8;
    for i in 0..8 {
        digitalWrite(clock_pin, true)?;
        let bit_val = if digitalRead(data_pin)? { 1u8 } else { 0u8 };
        digitalWrite(clock_pin, false)?;

        match order {
            BitOrder::LsbFirst => value |= bit_val << i,
            BitOrder::MsbFirst => value |= bit_val << (7 - i),
        }
    }
    Ok(value)
}

pub fn shiftOut(data_pin: &str, clock_pin: &str, order: BitOrder, value: u8) -> Result<(), CmdError> {
    for i in 0..8 {
        let bit_is_set = match order {
            BitOrder::LsbFirst => ((value >> i) & 0x01) != 0,
            BitOrder::MsbFirst => ((value >> (7 - i)) & 0x01) != 0,
        };
        digitalWrite(data_pin, bit_is_set)?;
        digitalWrite(clock_pin, true)?;
        digitalWrite(clock_pin, false)?;
    }
    Ok(())
}

pub fn isAlpha(c: char) -> bool {
    c.is_ascii_alphabetic()
}

pub fn isAlphaNumeric(c: char) -> bool {
    c.is_ascii_alphanumeric()
}

pub fn isAscii(c: char) -> bool {
    c.is_ascii()
}

pub fn isControl(c: char) -> bool {
    c.is_ascii_control()
}

pub fn isDigit(c: char) -> bool {
    c.is_ascii_digit()
}

pub fn isGraph(c: char) -> bool {
    c.is_ascii_graphic()
}

pub fn isHexadecimalDigit(c: char) -> bool {
    c.is_ascii_hexdigit()
}

pub fn isLowerCase(c: char) -> bool {
    c.is_ascii_lowercase()
}

pub fn isPrintable(c: char) -> bool {
    c.is_ascii_graphic() || c == ' '
}

pub fn isPunct(c: char) -> bool {
    c.is_ascii_punctuation()
}

pub fn isSpace(c: char) -> bool {
    c == ' '
}

pub fn isUpperCase(c: char) -> bool {
    c.is_ascii_uppercase()
}

pub fn isWhitespace(c: char) -> bool {
    c.is_whitespace()
}

pub fn interrupts() {
    INTERRUPTS_ENABLED.store(true, Ordering::SeqCst);
}

pub fn noInterrupts() {
    INTERRUPTS_ENABLED.store(false, Ordering::SeqCst);
}

pub fn interruptsEnabled() -> bool {
    INTERRUPTS_ENABLED.load(Ordering::SeqCst)
}

pub fn delay(ms: u64) {
    thread::sleep(Duration::from_millis(ms));
}

pub fn delayMicroseconds(us: u64) {
    thread::sleep(Duration::from_micros(us));
}

pub fn micros() -> u128 {
    start_time().elapsed().as_micros()
}

pub fn millis() -> u128 {
    start_time().elapsed().as_millis()
}

pub fn random(max_exclusive: i64) -> i64 {
    if max_exclusive <= 0 {
        return 0;
    }
    let mut rng = rng_store().lock().expect("rng lock poisoned");
    rng.gen_range(0..max_exclusive)
}

pub fn randomRange(min_inclusive: i64, max_exclusive: i64) -> i64 {
    if max_exclusive <= min_inclusive {
        return min_inclusive;
    }
    let mut rng = rng_store().lock().expect("rng lock poisoned");
    rng.gen_range(min_inclusive..max_exclusive)
}

pub fn randomSeed(seed: u64) {
    let mut rng = rng_store().lock().expect("rng lock poisoned");
    *rng = StdRng::seed_from_u64(seed);
}

pub fn WiFiOverview() -> Result<String, CmdError> {
    let output = Command::new("nmcli")
        .arg("-t")
        .arg("-f")
        .arg("STATE,CONNECTIVITY")
        .arg("general")
        .arg("status")
        .output()
        .map_err(|e| CmdError::Backend(format!("failed to run nmcli: {e}")))?;

    if !output.status.success() {
        return Err(CmdError::Backend(format!(
            "nmcli failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn scanWiFiNetworks() -> Result<Vec<WiFiNetwork>, CmdError> {
    let output = Command::new("nmcli")
        .arg("-t")
        .arg("-f")
        .arg("SSID,SIGNAL,SECURITY")
        .arg("dev")
        .arg("wifi")
        .arg("list")
        .output()
        .map_err(|e| CmdError::Backend(format!("failed to run nmcli: {e}")))?;

    if !output.status.success() {
        return Err(CmdError::Backend(format!(
            "nmcli wifi list failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut networks = Vec::new();

    for line in stdout.lines() {
        let mut parts = line.split(':');
        let ssid = parts.next().unwrap_or_default().to_string();
        let signal_dbm = parts.next().and_then(|x| x.parse::<i32>().ok());
        let security = parts
            .next()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);

        if !ssid.is_empty() {
            networks.push(WiFiNetwork {
                ssid,
                signal_dbm,
                security,
            });
        }
    }

    Ok(networks)
}
