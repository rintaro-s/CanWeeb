/**
 * Marrio - Arduino (HC-SR04 超音波センサ)
 *
 * 配線:
 *   TRIG → ピン 12
 *   ECHO → ピン 13
 *   VCC  → 5V
 *   GND  → GND
 *
 * シリアル出力 (9600 baud):
 *   距離 [cm] を 1 行ずつ送信 (例: "24.50")
 *   測定エラー時は "-1" を送信
 */

#define TRIG_PIN 12
#define ECHO_PIN 13
#define BAUD_RATE 9600
#define MEASURE_INTERVAL_MS 50
#define MAX_DISTANCE_CM 400.0
#define SOUND_SPEED_CM_US 0.0343

void setup() {
  Serial.begin(BAUD_RATE);
  pinMode(TRIG_PIN, OUTPUT);
  pinMode(ECHO_PIN, INPUT);
  digitalWrite(TRIG_PIN, LOW);
  delay(100);
}

void loop() {
  float distance = measureDistance();
  if (distance < 0) {
    Serial.println("-1");
  } else {
    Serial.println(distance, 2);
  }
  delay(MEASURE_INTERVAL_MS);
}

/**
 * HC-SR04 で距離を測定する。
 * @return 距離 [cm]。タイムアウト時は -1.0。
 */
float measureDistance() {
  // トリガーパルス送出 (10 μs)
  digitalWrite(TRIG_PIN, LOW);
  delayMicroseconds(2);
  digitalWrite(TRIG_PIN, HIGH);
  delayMicroseconds(10);
  digitalWrite(TRIG_PIN, LOW);

  // エコーパルス計測 (最大 30 ms 待機)
  long duration = pulseIn(ECHO_PIN, HIGH, 30000UL);
  if (duration == 0) {
    return -1.0;
  }

  float distance = (duration * SOUND_SPEED_CM_US) / 2.0;
  if (distance > MAX_DISTANCE_CM) {
    return -1.0;
  }

  return distance;
}
