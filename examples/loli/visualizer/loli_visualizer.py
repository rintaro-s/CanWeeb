#!/usr/bin/env python3
"""
Loli Visualizer - ロータリーエンコーダ位置ビジュアライザ (親)

CanWeeb から届いたロータリーエンコーダ情報を漏らさずキャッチし、
現在の方向をビジュアライズする。

初期位置を上（北）として、8方向で表示:
  N (北), NE (北東), E (東), SE (南東), S (南), SW (南西), W (西), NW (北西)

WebSocket で CanWeeb の /ws/inbox をリアルタイム監視し、
すべてのエンコーダ位置情報を確実に受信する。
"""

import os
import sys
import json
import time
import threading
from datetime import datetime

try:
    import websocket
    _HAS_WEBSOCKET = True
except ImportError:
    _HAS_WEBSOCKET = False
    print("警告: websocket-client がインストールされていません")
    print("  pip install websocket-client")
    sys.exit(1)

# 設定
CANWEEB_API = os.environ.get("CANWEEB_API", "http://localhost:8080")
WS_URL = CANWEEB_API.replace("http://", "ws://").replace("https://", "wss://") + "/ws/inbox"
STEPS_PER_DIRECTION = 4  # 1方向あたりのステップ数

# 8方向定義（初期位置は北）
DIRECTIONS = [
    ("N",  "北",   "↑"),
    ("NE", "北東", "↗"),
    ("E",  "東",   "→"),
    ("SE", "南東", "↘"),
    ("S",  "南",   "↓"),
    ("SW", "南西", "↙"),
    ("W",  "西",   "←"),
    ("NW", "北西", "↖"),
]

class EncoderVisualizer:
    def __init__(self):
        self.position = 0
        self.total_received = 0
        self.lock = threading.Lock()
        self.running = True
        self.last_update = None

    def get_direction(self):
        """現在の位置から方向を計算"""
        # 8方向に正規化
        total_steps = len(DIRECTIONS) * STEPS_PER_DIRECTION
        normalized = self.position % total_steps
        direction_index = normalized // STEPS_PER_DIRECTION
        return DIRECTIONS[direction_index]

    def update_position(self, position, delta):
        """位置を更新"""
        with self.lock:
            self.position = position
            self.total_received += 1
            self.last_update = datetime.now()

    def display(self):
        """現在の方向を表示"""
        with self.lock:
            code, name, arrow = self.get_direction()
            print(f"\r[受信: {self.total_received:>6}] 位置: {self.position:>6}  方向: {arrow} {name} ({code})  ", end="", flush=True)

    def display_full(self):
        """詳細表示"""
        with self.lock:
            code, name, arrow = self.get_direction()
            print("\n" + "="*60)
            print(f"  位置: {self.position}")
            print(f"  方向: {arrow} {name} ({code})")
            print(f"  受信総数: {self.total_received}")
            if self.last_update:
                print(f"  最終更新: {self.last_update.strftime('%H:%M:%S.%f')[:-3]}")
            print("="*60)

def on_message(ws, message):
    """WebSocket メッセージ受信時のコールバック"""
    try:
        data = json.loads(message)
        topic = data.get("topic", "")
        
        if topic == "loli/encoder/position":
            # payload_preview から直接パース
            preview = data.get("preview", "")
            try:
                payload = json.loads(preview)
                position = payload.get("position", 0)
                delta = payload.get("delta", 0)
                
                # 位置を更新
                visualizer.update_position(position, delta)
                visualizer.display()
                
            except json.JSONDecodeError:
                pass
    except Exception as e:
        print(f"\nメッセージ処理エラー: {e}", file=sys.stderr)

def on_error(ws, error):
    """WebSocket エラー時のコールバック"""
    print(f"\nWebSocket エラー: {error}", file=sys.stderr)

def on_close(ws, close_status_code, close_msg):
    """WebSocket 切断時のコールバック"""
    print(f"\nWebSocket 切断 (code={close_status_code}, msg={close_msg})")

def on_open(ws):
    """WebSocket 接続時のコールバック"""
    print(f"WebSocket 接続成功: {WS_URL}")
    print("エンコーダ位置情報を監視中...")
    print("Ctrl+C で終了")
    print("")

def run_websocket():
    """WebSocket クライアントを実行"""
    websocket.enableTrace(False)
    ws = websocket.WebSocketApp(
        WS_URL,
        on_open=on_open,
        on_message=on_message,
        on_error=on_error,
        on_close=on_close,
    )
    ws.run_forever()

def main():
    global visualizer
    
    print("="*60)
    print("  Loli Visualizer - ロータリーエンコーダ位置ビジュアライザ")
    print("="*60)
    print(f"  CANWEEB_API: {CANWEEB_API}")
    print(f"  WebSocket:   {WS_URL}")
    print(f"  方向数:      {len(DIRECTIONS)} 方向")
    print(f"  ステップ/方向: {STEPS_PER_DIRECTION}")
    print("="*60)
    print("")
    
    visualizer = EncoderVisualizer()
    
    # WebSocket スレッドを起動
    ws_thread = threading.Thread(target=run_websocket, daemon=True)
    ws_thread.start()
    
    try:
        # メインループ（定期的に詳細表示）
        while True:
            time.sleep(5)
            visualizer.display_full()
    except KeyboardInterrupt:
        print("\n\n終了します...")
        visualizer.running = False

if __name__ == "__main__":
    main()
