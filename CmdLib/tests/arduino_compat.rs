use canweeb_cmdlib::arduino::*;
use canweeb_cmdlib::prelude::*;
use std::sync::{Mutex, OnceLock};
use std::thread;

fn test_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .expect("test lock poisoned")
}

#[test]
fn digital_and_analog_work() {
    let _guard = test_guard();
    use_sim_backend().expect("switch backend failed");

    pinMode("gpio6", PinMode::Output).expect("pinMode failed");
    digitalWrite("gpio6", true).expect("digitalWrite failed");
    assert!(digitalRead("gpio6").expect("digitalRead failed"));

    analogReadResolution(12).expect("analogReadResolution failed");
    analogWriteResolution(10).expect("analogWriteResolution failed");
    analogReference(AnalogReference::Internal).expect("analogReference failed");
    analogWrite(2, 512).expect("analogWrite failed");
    let v = analogRead(2).expect("analogRead failed");
    assert!(v <= 4095);
}

#[test]
fn math_bits_chars_and_time_work() {
    assert_eq!(abs(-7_i32), 7_i32);
    assert_eq!(constrain(42, 0, 10), 10);
    assert_eq!(map(5, 0, 10, 0, 100), 50);
    assert_eq!(max(3, 9), 9);
    assert_eq!(min(3, 9), 3);
    assert_eq!(sq(12_i32), 144_i32);
    assert!((sqrt(81.0_f64) - 9.0).abs() < 1e-9);
    assert!((pow(2.0_f64, 8.0_f64) - 256.0).abs() < 1e-9);

    assert_eq!(bit(3), 0b1000);
    assert!(bitRead(0b1000, 3));
    assert_eq!(bitClear(0b1111, 1), 0b1101);
    assert_eq!(bitSet(0b0001, 2), 0b0101);
    assert_eq!(bitWrite(0b0000, 0, true), 0b0001);
    assert_eq!(highByte(0xABCD), 0xAB);
    assert_eq!(lowByte(0xABCD), 0xCD);

    assert!(isAlpha('A'));
    assert!(isAlphaNumeric('9'));
    assert!(isAscii('x'));
    assert!(isControl('\n'));
    assert!(isDigit('5'));
    assert!(isGraph('!'));
    assert!(isHexadecimalDigit('F'));
    assert!(isLowerCase('m'));
    assert!(isPrintable(' '));
    assert!(isPunct('.'));
    assert!(isSpace(' '));
    assert!(isUpperCase('Q'));
    assert!(isWhitespace('\t'));

    let m0 = millis();
    delay(2);
    let m1 = millis();
    assert!(m1 >= m0);

    let u0 = micros();
    delayMicroseconds(200);
    let u1 = micros();
    assert!(u1 >= u0);
}

#[test]
fn random_and_interrupts_work() {
    randomSeed(1234);
    let a = random(1000);
    randomSeed(1234);
    let b = random(1000);
    assert_eq!(a, b);

    let r = randomRange(10, 20);
    assert!((10..20).contains(&r));

    noInterrupts();
    assert!(!interruptsEnabled());
    interrupts();
    assert!(interruptsEnabled());
}

#[test]
fn shift_and_pulse_work() {
    let _guard = test_guard();
    use_sim_backend().expect("switch backend failed");

    pinMode("data", PinMode::Output).expect("pinMode data failed");
    pinMode("clk", PinMode::Output).expect("pinMode clk failed");
    shiftOut("data", "clk", BitOrder::MsbFirst, 0b1010_0101).expect("shiftOut failed");

    pinMode("pulse", PinMode::Output).expect("pinMode pulse failed");
    digitalWrite("pulse", true).expect("set pulse failed");
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(2));
        let _ = digitalWrite("pulse", false);
    });
    let p = pulseInLong("pulse", true, 1_000_000).expect("pulseInLong failed");
    assert!(p > 0);
}

#[test]
fn wifi_client_server_udp_work() {
    let server = WiFiServer::bind("127.0.0.1:0").expect("WiFiServer bind failed");
    let addr = server.local_addr().expect("local_addr failed");

    let server_handle = thread::spawn(move || {
        let (mut client, _peer) = server.accept().expect("accept failed");
        let recv = client.read_bytes(64).expect("server read failed");
        assert_eq!(recv, b"PING");
        client.println("PONG").expect("server println failed");
    });

    let mut client = WiFiClient::connect(&addr.to_string()).expect("WiFiClient connect failed");
    client.write_bytes(b"PING").expect("client write failed");
    let resp = client.read_bytes(64).expect("client read failed");
    assert!(String::from_utf8_lossy(&resp).contains("PONG"));
    server_handle.join().expect("server thread panic");

    let udp_a = WiFiUDP::bind("127.0.0.1:0").expect("udp a bind failed");
    let udp_b = WiFiUDP::bind("127.0.0.1:0").expect("udp b bind failed");
    let b_addr = udp_b.local_addr().expect("udp b local addr failed");
    udp_a
        .send_to(b"HELLO", &b_addr.to_string())
        .expect("udp send failed");
    let (pkt, _) = udp_b.recv_from(64).expect("udp recv failed");
    assert_eq!(pkt, b"HELLO");
}
