#!/usr/bin/env python3
"""
Marrio - PC ゲームプログラム
=============================
操作:
  - 顔の位置 (Webカメラ) → 画面上のプレイヤーのX/Y位置
  - 口パク (口の開閉) → アイテム取得
  - CanWeeb topic "marrio/input/jump"  → ジャンプ (RasPi-A / Arduino)
  - CanWeeb topic "marrio/input/move"  → 左右移動 (RasPi-B / STM32)

依存:
  pip install pygame opencv-python mediapipe requests numpy

環境変数:
  CANWEEB_API   CanWeeb API URL (default: http://localhost:8080)
  CAMERA_INDEX  カメラインデックス (default: 0)
"""

import sys
import os
import math
import random
import time
import threading
import json
import base64
import requests
import pygame
import cv2
import numpy as np

# websocket-client ライブラリ
try:
    import websocket
    _HAS_WEBSOCKET = True
except ImportError:
    _HAS_WEBSOCKET = False

# pygame.mixer は使用しない（依存関係削減）
from dataclasses import dataclass, field
from typing import List, Optional, Tuple
from enum import Enum, auto

# ---------------------------------------------------------------------------
# 設定
# ---------------------------------------------------------------------------
CANWEEB_API   = os.environ.get("CANWEEB_API", "http://localhost:8080")
CAMERA_INDEX  = int(os.environ.get("CAMERA_INDEX", "0"))
POLL_INTERVAL = 0.05          # CWB ポーリングフォールバック間隔 (秒)
MOVE_HOLD_SEC = 0.50          # 移動イベントを何秒間有効とみなすか
SCREEN_W, SCREEN_H = 1280, 720
FPS           = 60
GRAVITY       = 0.55
JUMP_VY       = -13.0
PLAYER_SPEED  = 5.0
TILE_W, TILE_H = 48, 48
FACE_PREVIEW_W, FACE_PREVIEW_H = 240, 180   # 右下に表示するカメラ縮小サイズ
MOUTH_OPEN_THRESHOLD = 0.35   # 口開閉の面積比率閾値

# ---------------------------------------------------------------------------
# 色
# ---------------------------------------------------------------------------
SKY       = (107, 140, 255)
GROUND_C  = (139,  90,  43)
BRICK_C   = (180,  80,  20)
COIN_C    = (255, 215,   0)
ENEMY_C   = (220,  50,  50)
PLAYER_C  = (255, 120,  30)
FLAG_C    = (255, 255, 255)
HUD_BG    = (0, 0, 0, 160)
WHITE     = (255, 255, 255)
BLACK     = (0,   0,   0)
RED       = (220,  50,  50)
GREEN     = ( 50, 200,  50)
YELLOW    = (255, 220,   0)
DARK_BLUE = ( 20,  20,  80)

# ---------------------------------------------------------------------------
# ゲーム定数
# ---------------------------------------------------------------------------
LEVEL_WIDTH_TILES = 160
COIN_SCORE    = 100
ENEMY_SCORE   = 200
STOMP_VY      = -9.0
PLAYER_LIVES  = 3

# ---------------------------------------------------------------------------
# CanWeeb クライアント (WebSocket優先 / HTTPポーリングフォールバック)
# ---------------------------------------------------------------------------
class CanWeebClient:
    """Marrio 用の CWB イベント受信クライアント。

    raspi からの jump / move は Control メッセージとして送る。
    そのため realtime 受信経路は /ws/inbox、フォールバックは /api/inbox を使う。
    """

    def __init__(self, api_base: str):
        self.api_base = api_base.rstrip("/")
        self._lock           = threading.Lock()
        self._jump_pending   = False
        self._move_dir: Optional[str] = None
        self._move_expires: float     = 0.0
        self._seen_ids: set[str] = set()
        self._newest_inbox_ms: int = 0
        self._running = True

        self._initialize_inbox_watermark()

        if _HAS_WEBSOCKET:
            t = threading.Thread(target=self._ws_loop, daemon=True)
            print("[CWB] WebSocket モードで起動")
        else:
            t = threading.Thread(target=self._poll_loop, daemon=True)
            print("[CWB] HTTP ポーリングモードで起動 (pip install websocket-client 推奨)")
        t.start()

    # ---- 公開 API ----

    def consume_jump(self) -> bool:
        with self._lock:
            v = self._jump_pending
            self._jump_pending = False
            return v

    def get_move(self) -> Optional[str]:
        """有効期限内の移動方向を返す。期限切れ/未受信は None。"""
        with self._lock:
            if self._move_dir and time.monotonic() < self._move_expires:
                return self._move_dir
            self._move_dir = None
            return None

    def stop(self):
        self._running = False

    def _initialize_inbox_watermark(self):
        try:
            resp = requests.get(f"{self.api_base}/api/inbox", timeout=2.0)
            if resp.status_code != 200:
                return
            items = resp.json()
            relevant = [
                item.get("received_at_ms", 0)
                for item in items
                if item.get("subject") in ("jump", "move")
            ]
            if relevant:
                self._newest_inbox_ms = max(relevant)
            print(f"[CWB] inbox watermark = {self._newest_inbox_ms}")
        except Exception as e:
            print(f"[CWB] inbox watermark 初期化失敗: {e}")

    # ---- WebSocket ループ ----

    def _ws_loop(self):
        ws_url = (self.api_base
                  .replace("https://", "wss://")
                  .replace("http://",  "ws://")) + "/ws/inbox"
        while self._running:
            try:
                ws = websocket.WebSocket()
                ws.settimeout(15)          # recv タイムアウト (keepalive 用)
                ws.connect(ws_url)
                print(f"[CWB] WS 接続: {ws_url}")
                while self._running:
                    try:
                        raw = ws.recv()
                        if raw:
                            self._handle_ws_msg(raw)
                    except websocket.WebSocketTimeoutException:
                        continue   # タイムアウトは正常 → 再受信
                    except Exception as e:
                        print(f"[CWB] WS 切断: {e}")
                        break
                try:
                    ws.close()
                except Exception:
                    pass
            except Exception as e:
                print(f"[CWB] WS 接続失敗: {e}")
            if self._running:
                time.sleep(3.0)

    def _handle_ws_msg(self, raw: str):
        try:
            msg   = json.loads(raw)
            self._handle_inbox_event(msg)
        except Exception as e:
            print(f"[CWB] WS メッセージ処理エラー: {e}")

    # ---- HTTP ポーリングフォールバック ----

    def _poll_loop(self):
        while self._running:
            try:
                self._poll_inbox()
            except Exception:
                pass
            time.sleep(max(POLL_INTERVAL, 0.2))

    def _poll_inbox(self):
        try:
            resp = requests.get(f"{self.api_base}/api/inbox", timeout=1.5)
            if resp.status_code != 200:
                return
            items = resp.json()
            fresh = [
                item for item in items
                if item.get("subject") in ("jump", "move")
                and item.get("message_id") not in self._seen_ids
                and item.get("received_at_ms", 0) > self._newest_inbox_ms
            ]
            fresh.sort(key=lambda item: item.get("received_at_ms", 0))
            for item in fresh:
                self._handle_inbox_event(item)
        except Exception:
            pass

    def _handle_inbox_event(self, item: dict):
        try:
            message_id = item.get("message_id")
            if message_id:
                self._seen_ids.add(message_id)
            self._newest_inbox_ms = max(self._newest_inbox_ms, item.get("received_at_ms", 0))

            subject = item.get("subject", "")
            preview = item.get("preview", "")
            payload = json.loads(preview) if preview else {}

            if subject == "jump":
                with self._lock:
                    self._jump_pending = True
                print(f"[CWB] >>> JUMP 受信 from {item.get('source_node')}")
            elif subject == "move":
                direction = payload.get("direction", "")
                if direction not in ("left", "right"):
                    return
                with self._lock:
                    self._move_dir     = direction
                    self._move_expires = time.monotonic() + MOVE_HOLD_SEC
                print(f"[CWB] >>> MOVE 受信 from {item.get('source_node')}: {direction}")
        except Exception:
            pass


# ---------------------------------------------------------------------------
# 顔追跡 (OpenCV HaarCascade - mediapipe不要)
# ---------------------------------------------------------------------------
class FaceTracker:
    def __init__(self, camera_index: int = 0):
        self.cap = cv2.VideoCapture(camera_index)
        if not self.cap.isOpened():
            raise RuntimeError(f"カメラ {camera_index} を開けません")
        self.cap.set(cv2.CAP_PROP_FRAME_WIDTH,  640)
        self.cap.set(cv2.CAP_PROP_FRAME_HEIGHT, 480)
        self.cap.set(cv2.CAP_PROP_FPS, 30)

        # HaarCascade ロード (OpenCV 同梱)
        cascade_dir = cv2.data.haarcascades
        self._face_cascade  = cv2.CascadeClassifier(
            cascade_dir + "haarcascade_frontalface_default.xml")
        self._mouth_cascade = cv2.CascadeClassifier(
            cascade_dir + "haarcascade_smile.xml")

        self._lock       = threading.Lock()
        self._face_x     = 0.5
        self._face_y     = 0.5
        self._mouth_open = False
        self._frame_rgb: Optional[np.ndarray] = None
        self._running = True
        self._thread  = threading.Thread(target=self._capture_loop, daemon=True)
        self._thread.start()
        print("[FaceTracker] OpenCV HaarCascade で起動")

    def get_state(self) -> Tuple[float, float, bool, Optional[np.ndarray]]:
        with self._lock:
            return self._face_x, self._face_y, self._mouth_open, self._frame_rgb

    def stop(self):
        self._running = False
        self.cap.release()

    def _capture_loop(self):
        while self._running:
            ret, frame = self.cap.read()
            if not ret:
                time.sleep(0.05)
                continue

            frame = cv2.flip(frame, 1)  # 左右反転（鏡像）
            h, w  = frame.shape[:2]
            gray  = cv2.cvtColor(frame, cv2.COLOR_BGR2GRAY)
            gray  = cv2.equalizeHist(gray)

            face_x, face_y = 0.5, 0.5
            mouth_open = False

            faces = self._face_cascade.detectMultiScale(
                gray, scaleFactor=1.1, minNeighbors=4, minSize=(80, 80))

            if len(faces) > 0:
                # 最大の顔を使用
                fx, fy, fw, fh = max(faces, key=lambda r: r[2] * r[3])
                face_x = (fx + fw / 2) / w
                face_y = (fy + fh / 2) / h

                # 顔領域の下半分で口を検出
                roi_y  = fy + fh // 2
                roi_h  = fh // 2
                roi    = gray[roi_y:roi_y + roi_h, fx:fx + fw]
                smiles = self._mouth_cascade.detectMultiScale(
                    roi, scaleFactor=1.7, minNeighbors=22, minSize=(25, 15))
                mouth_open = len(smiles) > 0

            # プレビュー用縮小フレーム (RGB)
            rgb   = cv2.cvtColor(frame, cv2.COLOR_BGR2RGB)
            small = cv2.resize(rgb, (FACE_PREVIEW_W, FACE_PREVIEW_H))

            with self._lock:
                self._face_x     = face_x
                self._face_y     = face_y
                self._mouth_open = mouth_open
                self._frame_rgb  = small


# ---------------------------------------------------------------------------
# タイルマップ
# ---------------------------------------------------------------------------
class TileType(Enum):
    EMPTY  = 0
    GROUND = 1
    BRICK  = 2
    COIN   = 3  # コインブロック (取れる)
    PIPE   = 4  # 土管
    FLAG   = 5  # ゴールフラグ

@dataclass
class Tile:
    ttype: TileType
    collected: bool = False  # コインブロックを取得済みか

@dataclass
class Coin:
    x: float
    y: float
    alive: bool = True
    vy: float   = -4.0        # ポップアップ演出用

@dataclass
class Enemy:
    x: float
    y: float
    vx: float  = -1.5
    vy: float  = 0.0
    alive: bool = True
    stomped: bool = False
    stomp_timer: float = 0.0
    W: int = field(default=TILE_W - 6, init=False)
    H: int = field(default=TILE_H - 4, init=False)

@dataclass
class Particle:
    x: float
    y: float
    vx: float
    vy: float
    life: float        # 残り秒数
    color: Tuple[int, int, int]
    size: int = 4


# ---------------------------------------------------------------------------
# レベル生成
# ---------------------------------------------------------------------------
def build_level() -> List[List[Tile]]:
    W = LEVEL_WIDTH_TILES
    H = 16
    tiles = [[Tile(TileType.EMPTY) for _ in range(W)] for _ in range(H)]

    # 地面 (最下段2列)
    for x in range(W):
        tiles[H - 1][x] = Tile(TileType.GROUND)
        tiles[H - 2][x] = Tile(TileType.GROUND)

    # ランダムな台地
    random.seed(42)
    platforms = [
        (5, H - 5, 4), (12, H - 6, 3), (18, H - 4, 5),
        (26, H - 6, 4), (33, H - 5, 3), (40, H - 7, 4),
        (48, H - 5, 5), (56, H - 6, 3), (63, H - 4, 6),
        (72, H - 6, 4), (80, H - 5, 3), (88, H - 7, 5),
        (96, H - 5, 4), (104, H - 6, 3),(112, H - 4, 5),
        (120, H - 6, 4),(128, H - 5, 6),(136, H - 7, 4),
        (144, H - 5, 3),(150, H - 4, 5),
    ]
    for (px, py, pw) in platforms:
        for x in range(px, min(px + pw, W)):
            tiles[py][x] = Tile(TileType.BRICK)

    # コインブロック
    coin_positions = [
        (7, H-6), (9, H-6), (14, H-8), (20, H-6),
        (28, H-8), (35, H-7), (42, H-9), (50, H-7),
        (58, H-8), (65, H-6), (74, H-8), (82, H-7),
        (90, H-9), (98, H-7), (106, H-8),(114, H-6),
        (122, H-8),(130, H-7),(138, H-9),(146, H-7),
    ]
    for (cx, cy) in coin_positions:
        if 0 <= cy < H and 0 <= cx < W:
            tiles[cy][cx] = Tile(TileType.COIN)

    # 土管
    pipe_positions = [16, 30, 46, 60, 78, 94, 108, 124, 140, 154]
    for px in pipe_positions:
        if px + 1 < W:
            for row in range(H - 4, H - 2):
                tiles[row][px]     = Tile(TileType.PIPE)
                tiles[row][px + 1] = Tile(TileType.PIPE)

    # ゴールフラグ (最後尾)
    fx = W - 4
    for row in range(H - 8, H - 2):
        tiles[row][fx] = Tile(TileType.FLAG)

    return tiles


def build_enemies(level_h: int) -> List[Enemy]:
    random.seed(123)
    enemies: List[Enemy] = []
    spawn_xs = [8, 15, 22, 35, 44, 53, 67, 76, 85, 98,
                107, 116, 127, 136, 145, 152]
    H = level_h
    for ex in spawn_xs:
        ey = (H - 3) * TILE_H
        enemies.append(Enemy(x=float(ex * TILE_H), y=float(ey), vx=random.choice([-1.5, -2.0])))
    return enemies


# ---------------------------------------------------------------------------
# プレイヤー
# ---------------------------------------------------------------------------
@dataclass
class Player:
    x: float = 2.0 * TILE_W
    y: float = 10.0 * TILE_H
    vx: float = 0.0
    vy: float = 0.0
    on_ground: bool = False
    alive: bool = True
    dead_timer: float = 0.0
    score: int = 0
    lives: int = PLAYER_LIVES
    coins_collected: int = 0
    invincible: float = 0.0  # 無敵時間(秒)
    W: int = field(default=TILE_W - 6, init=False)
    H: int = field(default=TILE_H - 4, init=False)

    def rect(self):
        return pygame.Rect(int(self.x), int(self.y), self.W, self.H)

@dataclass
class FaceEntity:
    """顔追跡による独立エンティティ（アイテム取得専用）"""
    screen_x: float = SCREEN_W / 2
    screen_y: float = SCREEN_H / 2
    face_x: float = 0.5   # カメラ座標 0-1
    face_y: float = 0.5
    mouth_open: bool = False
    size: int = 32  # 表示サイズ


# ---------------------------------------------------------------------------
# ゲームステート
# ---------------------------------------------------------------------------
class GameState(Enum):
    TITLE    = auto()
    PLAYING  = auto()
    DEAD     = auto()
    CLEARED  = auto()
    GAMEOVER = auto()


# ---------------------------------------------------------------------------
# メインゲームクラス
# ---------------------------------------------------------------------------
class MarrioGame:
    def __init__(self):
        pygame.init()
        self.screen = pygame.display.set_mode((SCREEN_W, SCREEN_H))
        pygame.display.set_caption("Marrio - Face Control Edition")
        self.clock  = pygame.time.Clock()

        # テキスト描画は _draw_simple_text (ピクセル描画) で行う。
        # pygame.font は Python 3.14 + pygame 2.6.1 で circular import を起こすため使用しない。

        # CanWeeb クライアント
        self.cwb = CanWeebClient(CANWEEB_API)

        # 顔追跡
        try:
            self.face_tracker: Optional[FaceTracker] = FaceTracker(CAMERA_INDEX)
            print("[FaceTracker] カメラ初期化成功")
        except Exception as e:
            print(f"[FaceTracker] カメラ初期化失敗: {e} → マウス操作に切り替え")
            self.face_tracker = None

        self.state       = GameState.TITLE
        self.high_score  = 0
        self._init_game()

    # ------------------------------------------------------------------
    # ゲーム初期化
    # ------------------------------------------------------------------
    def _init_game(self):
        self.tiles    = build_level()
        self.level_h  = len(self.tiles)
        self.level_w  = len(self.tiles[0])
        self.enemies  = build_enemies(self.level_h)
        self.coins: List[Coin]       = []
        self.particles: List[Particle] = []
        self.player   = Player()
        self.face     = FaceEntity()  # 顔エンティティ（独立）
        self.camera_x = 0.0
        self.goal_reached = False
        self.goal_timer   = 0.0


    # ------------------------------------------------------------------
    # タイル衝突ユーティリティ
    # ------------------------------------------------------------------
    def _tile_at(self, tx: int, ty: int) -> TileType:
        if ty < 0 or ty >= self.level_h or tx < 0 or tx >= self.level_w:
            return TileType.EMPTY
        return self.tiles[ty][tx].ttype

    def _is_solid(self, tx: int, ty: int) -> bool:
        t = self._tile_at(tx, ty)
        return t in (TileType.GROUND, TileType.BRICK, TileType.PIPE, TileType.FLAG, TileType.COIN)

    def _resolve_collision(self, entity, vx: float, vy: float,
                            is_player: bool = False) -> Tuple[float, float, bool, bool]:
        """AABB タイル衝突解決。返り値: (new_vx, new_vy, on_ground, hit_ceiling)"""
        EW = entity.W
        EH = entity.H
        ex, ey = entity.x, entity.y
        on_ground   = False
        hit_ceiling = False

        # 横移動
        ex += vx
        left_tile  = int(ex // TILE_W)
        right_tile = int((ex + EW - 1) // TILE_W)
        for ty in range(int(ey // TILE_H), int((ey + EH - 1) // TILE_H) + 1):
            if vx > 0 and self._is_solid(right_tile, ty):
                ex = right_tile * TILE_W - EW
                vx = 0
                break
            if vx < 0 and self._is_solid(left_tile, ty):
                ex = (left_tile + 1) * TILE_W
                vx = 0
                break

        # 縦移動
        ey += vy
        top_tile    = int(ey // TILE_H)
        bottom_tile = int((ey + EH - 1) // TILE_H)
        left_tile   = int(ex // TILE_W)
        right_tile  = int((ex + EW - 1) // TILE_W)

        if vy > 0:  # 落下
            for tx in range(left_tile, right_tile + 1):
                if self._is_solid(tx, bottom_tile):
                    ey = bottom_tile * TILE_H - EH
                    vy = 0
                    on_ground = True
                    break
        elif vy < 0:  # 上昇
            for tx in range(left_tile, right_tile + 1):
                if self._is_solid(tx, top_tile):
                    ey = (top_tile + 1) * TILE_H
                    vy = 0
                    hit_ceiling = True
                    if is_player:  # ブロック破壊はプレイヤーのみ
                        self._hit_block(tx, top_tile, entity)
                    break

        entity.x, entity.y = ex, ey
        return vx, vy, on_ground, hit_ceiling

    def _hit_block(self, tx: int, ty: int, player: Player):
        tile = self.tiles[ty][tx]
        if tile.ttype == TileType.COIN and not tile.collected:
            tile.collected = True
            tile.ttype = TileType.BRICK  # 空になったブロック
            # コインをポップ
            self.coins.append(Coin(x=tx * TILE_W + TILE_W / 2, y=ty * TILE_H))
            player.score += COIN_SCORE
            player.coins_collected += 1
            self._spawn_particles(tx * TILE_W + TILE_W / 2, ty * TILE_H, COIN_C, 8)

    # ------------------------------------------------------------------
    # パーティクル
    # ------------------------------------------------------------------
    def _spawn_particles(self, x: float, y: float, color, count: int):
        for _ in range(count):
            angle = random.uniform(0, math.tau)
            speed = random.uniform(2, 6)
            self.particles.append(Particle(
                x=x, y=y,
                vx=math.cos(angle) * speed,
                vy=math.sin(angle) * speed - 2,
                life=random.uniform(0.3, 0.7),
                color=color,
                size=random.randint(3, 6),
            ))

    # ------------------------------------------------------------------
    # メインループ
    # ------------------------------------------------------------------
    def run(self):
        while True:
            dt = self.clock.tick(FPS) / 1000.0
            dt = min(dt, 0.05)  # 最大 50ms

            for event in pygame.event.get():
                if event.type == pygame.QUIT:
                    self._quit()
                if event.type == pygame.KEYDOWN:
                    self._handle_key(event.key)

            if self.state == GameState.TITLE:
                self._update_title(dt)
                self._draw_title()
            elif self.state == GameState.PLAYING:
                self._update_game(dt)
                self._draw_game()
            elif self.state == GameState.DEAD:
                self._update_dead(dt)
                self._draw_game()
                self._draw_dead_overlay()
            elif self.state == GameState.CLEARED:
                self._update_cleared(dt)
                self._draw_game()
                self._draw_cleared_overlay()
            elif self.state == GameState.GAMEOVER:
                self._draw_gameover()

            pygame.display.flip()

    # ------------------------------------------------------------------
    # タイトル
    # ------------------------------------------------------------------
    def _update_title(self, dt: float):
        pass

    def _draw_title(self):
        self.screen.fill(DARK_BLUE)
        # タイトル（図形で簡易表示）
        pygame.draw.rect(self.screen, YELLOW, (SCREEN_W // 2 - 150, 160, 300, 60))
        pygame.draw.rect(self.screen, DARK_BLUE, (SCREEN_W // 2 - 145, 165, 290, 50))
        
        # 簡易テキスト表示（ドット）
        self._draw_simple_text("MARRIO", SCREEN_W // 2 - 100, 180, YELLOW, 4)
        self._draw_simple_text("SPACE TO START", SCREEN_W // 2 - 140, 400, WHITE, 2)

    # ------------------------------------------------------------------
    # ゲームアップデート
    # ------------------------------------------------------------------
    def _update_game(self, dt: float):
        player = self.player
        face   = self.face

        # ---- 入力収集 ----
        jump_cwb  = self.cwb.consume_jump()
        move_cwb  = self.cwb.get_move()   # 期限付き移動方向（None or "left"/"right"）

        keys = pygame.key.get_pressed()

        # ---- 顔追跡（独立エンティティ） ----
        if self.face_tracker:
            fx, fy, mouth_open, _ = self.face_tracker.get_state()
            face.face_x     = fx
            face.face_y     = fy
            face.mouth_open = mouth_open
        else:
            # フォールバック: マウス
            mx, my = pygame.mouse.get_pos()
            face.face_x     = mx / SCREEN_W
            face.face_y     = my / SCREEN_H
            face.mouth_open = pygame.mouse.get_pressed()[0]

        # 顔のスクリーン座標を更新（カメラ追従なし、画面固定）
        face.screen_x = face.face_x * SCREEN_W
        face.screen_y = face.face_y * SCREEN_H

        if not player.alive:
            return

        # ---- 水平方向の移動決定 ----
        # 優先度: CWB (ロータリーエンコーダ) > キーボード
        # ※顔は移動に影響しない
        target_vx = 0.0

        # CWB 移動イベント（ロータリーエンコーダ）
        if move_cwb == "left":
            target_vx = -PLAYER_SPEED
        elif move_cwb == "right":
            target_vx = PLAYER_SPEED

        # キーボード補助
        if keys[pygame.K_LEFT]:
            target_vx = -PLAYER_SPEED
        if keys[pygame.K_RIGHT]:
            target_vx = PLAYER_SPEED

        # 滑らか補間
        player.vx += (target_vx - player.vx) * 0.25
        if abs(player.vx) < 0.1:
            player.vx = 0.0

        # ---- ジャンプ（超音波センサ or キーボード） ----
        want_jump = (
            jump_cwb  # 超音波センサ（RasPi-A）
            or keys[pygame.K_SPACE]
            or keys[pygame.K_UP]
        )
        if want_jump and player.on_ground:
            player.vy = JUMP_VY
            player.on_ground = False
            self._spawn_particles(player.x + player.W / 2, player.y + player.H, WHITE, 6)

        # ---- 重力 ----
        player.vy += GRAVITY
        player.vy  = min(player.vy, 18.0)

        # ---- 衝突解決 ----
        vx, vy, on_ground, _ = self._resolve_collision(player, player.vx, player.vy, is_player=True)
        player.vx       = vx
        player.vy       = vy
        player.on_ground = on_ground

        # 落下死
        if player.y > self.level_h * TILE_H:
            self._player_die()
            return

        # 無敵時間
        if player.invincible > 0:
            player.invincible = max(0.0, player.invincible - dt)

        # ---- 敵の更新 ----
        for enemy in self.enemies:
            if not enemy.alive:
                continue
            if enemy.stomped:
                enemy.stomp_timer -= dt
                if enemy.stomp_timer <= 0:
                    enemy.alive = False
                continue

            enemy.vy += GRAVITY
            old_ex, old_ey = enemy.x, enemy.y
            # 壁反転
            enemy.vx, enemy.vy, _, _ = self._resolve_collision(enemy, enemy.vx, enemy.vy, is_player=False)
            if enemy.x == old_ex and enemy.vx == 0:
                enemy.vx = -enemy.vx if enemy.vx != 0 else 1.5

            # プレイヤーとの衝突
            if player.invincible <= 0:
                erect = pygame.Rect(int(enemy.x), int(enemy.y), enemy.W, enemy.H)
                if player.rect().colliderect(erect):
                    # 踏みつけ判定: プレイヤーが上から来た
                    if player.vy > 0 and player.y + player.H - 8 <= enemy.y + 4:
                        enemy.stomped    = True
                        enemy.stomp_timer = 0.4
                        player.vy        = STOMP_VY
                        player.score    += ENEMY_SCORE
                        self._spawn_particles(enemy.x + TILE_W / 2, enemy.y, RED, 10)
                    else:
                        self._player_die()
                        return

        # ---- コイン (ポップアップ) の更新 ----
        for coin in self.coins:
            if not coin.alive:
                continue
            coin.y  += coin.vy
            coin.vy += 0.5
            if coin.vy > 0 and coin.y > (self.level_h - 3) * TILE_H:
                coin.alive = False

        # ---- 口パクでコイン取得（顔エンティティ） ----
        if face.mouth_open:
            # 顔のワールド座標を計算（カメラオフセット込み）
            face_world_x = face.screen_x + self.camera_x
            face_world_y = face.screen_y
            
            for coin in self.coins:
                if not coin.alive:
                    continue
                dist = math.hypot(coin.x - face_world_x, coin.y - face_world_y)
                if dist < TILE_W * 2.5:  # 顔の取得範囲
                    coin.alive = False
                    player.score += COIN_SCORE
                    player.coins_collected += 1
                    self._spawn_particles(coin.x, coin.y, COIN_C, 6)

        # ---- マップ上コイン (タイルを直接触る) での取得 ----
        # (コインタイルへの接触)
        prect = player.rect()
        left_t  = int(player.x // TILE_W)
        right_t = int((player.x + player.W - 1) // TILE_W)
        top_t   = int(player.y // TILE_H)
        bot_t   = int((player.y + player.H - 1) // TILE_H)
        for ty in range(top_t, bot_t + 1):
            for tx in range(left_t, right_t + 1):
                if 0 <= ty < self.level_h and 0 <= tx < self.level_w:
                    tile = self.tiles[ty][tx]
                    if tile.ttype == TileType.COIN and not tile.collected:
                        tile.collected = True
                        tile.ttype = TileType.BRICK
                        player.score += COIN_SCORE
                        player.coins_collected += 1
                        self._spawn_particles(tx * TILE_W + TILE_W / 2, ty * TILE_H, COIN_C, 6)

        # ---- ゴールフラグ ----
        for ty in range(top_t, bot_t + 1):
            for tx in range(left_t, right_t + 1):
                if self._tile_at(tx, ty) == TileType.FLAG:
                    if not self.goal_reached:
                        self.goal_reached = True
                        self._spawn_particles(player.x, player.y, YELLOW, 30)
                    self.state = GameState.CLEARED
                    return

        # ---- パーティクル更新 ----
        for p in self.particles:
            p.x    += p.vx
            p.y    += p.vy
            p.vy   += 0.3
            p.life -= dt
        self.particles = [p for p in self.particles if p.life > 0]

        # ---- カメラ追従 ----
        target_cam_x = player.x - SCREEN_W // 3
        self.camera_x += (target_cam_x - self.camera_x) * 0.12
        self.camera_x  = max(0, min(self.camera_x, self.level_w * TILE_W - SCREEN_W))

    def _player_die(self):
        player = self.player
        player.alive    = False
        player.dead_timer = 2.0
        player.vy       = JUMP_VY * 1.2
        self._spawn_particles(player.x + player.W / 2, player.y, RED, 20)
        self.state = GameState.DEAD

    # ------------------------------------------------------------------
    # ゲーム描画
    # ------------------------------------------------------------------
    def _draw_game(self):
        self.screen.fill(SKY)

        cam_x = int(self.camera_x)
        start_tx = max(0, cam_x // TILE_W)
        end_tx   = min(self.level_w, (cam_x + SCREEN_W) // TILE_W + 2)

        # タイル描画
        for ty in range(self.level_h):
            for tx in range(start_tx, end_tx):
                tile = self.tiles[ty][tx]
                sx = tx * TILE_W - cam_x
                sy = ty * TILE_H
                if tile.ttype == TileType.GROUND:
                    pygame.draw.rect(self.screen, GROUND_C, (sx, sy, TILE_W, TILE_H))
                    pygame.draw.rect(self.screen, (100, 60, 20), (sx, sy, TILE_W, TILE_H), 1)
                elif tile.ttype == TileType.BRICK:
                    pygame.draw.rect(self.screen, BRICK_C, (sx, sy, TILE_W, TILE_H))
                    pygame.draw.line(self.screen, (140, 60, 10), (sx, sy + TILE_H // 2), (sx + TILE_W, sy + TILE_H // 2), 1)
                    pygame.draw.line(self.screen, (140, 60, 10), (sx + TILE_W // 2, sy), (sx + TILE_W // 2, sy + TILE_H // 2), 1)
                    pygame.draw.line(self.screen, (140, 60, 10), (sx, sy + TILE_H // 2), (sx, sy + TILE_H), 1)
                elif tile.ttype == TileType.COIN and not tile.collected:
                    pygame.draw.rect(self.screen, (200, 150, 20), (sx, sy, TILE_W, TILE_H))
                    pygame.draw.circle(self.screen, COIN_C, (sx + TILE_W // 2, sy + TILE_H // 2), TILE_W // 3)
                    self._draw_simple_text("?", sx + TILE_W // 2 - 5, sy + TILE_H // 2 - 5, BLACK, 2)
                elif tile.ttype == TileType.PIPE:
                    pygame.draw.rect(self.screen, (30, 160, 40), (sx, sy, TILE_W, TILE_H))
                    pygame.draw.rect(self.screen, (20, 120, 30), (sx, sy, TILE_W, TILE_H), 2)
                elif tile.ttype == TileType.FLAG:
                    pygame.draw.rect(self.screen, (180, 180, 180), (sx + TILE_W // 2 - 2, sy, 4, TILE_H))
                    pygame.draw.polygon(self.screen, (255, 80, 80), [
                        (sx + TILE_W // 2 + 2, sy + 4),
                        (sx + TILE_W // 2 + 20, sy + 12),
                        (sx + TILE_W // 2 + 2, sy + 20),
                    ])

        # 敵描画
        for enemy in self.enemies:
            if not enemy.alive:
                continue
            sx = int(enemy.x) - cam_x
            sy = int(enemy.y)
            ew = TILE_W - 6
            eh = TILE_H - 4
            if enemy.stomped:
                pygame.draw.ellipse(self.screen, ENEMY_C, (sx, sy + eh - 8, ew, 8))
            else:
                pygame.draw.ellipse(self.screen, ENEMY_C, (sx, sy, ew, eh))
                pygame.draw.circle(self.screen, WHITE, (sx + ew // 3, sy + eh // 3), 4)
                pygame.draw.circle(self.screen, WHITE, (sx + 2 * ew // 3, sy + eh // 3), 4)
                pygame.draw.circle(self.screen, BLACK, (sx + ew // 3, sy + eh // 3), 2)
                pygame.draw.circle(self.screen, BLACK, (sx + 2 * ew // 3, sy + eh // 3), 2)

        # ポップアップコイン
        for coin in self.coins:
            if not coin.alive:
                continue
            sx = int(coin.x) - cam_x
            sy = int(coin.y)
            pygame.draw.circle(self.screen, COIN_C, (sx, sy), 8)

        # パーティクル
        for p in self.particles:
            sx = int(p.x) - cam_x
            pygame.draw.circle(self.screen, p.color, (sx, int(p.y)), p.size)

        # プレイヤー描画（マリオ本体）
        player = self.player
        sx = int(player.x) - cam_x
        sy = int(player.y)
        # 無敵点滅
        if player.invincible <= 0 or int(player.invincible * 10) % 2 == 0:
            # 体
            pygame.draw.rect(self.screen, PLAYER_C, (sx, sy, player.W, player.H))
            # 帽子
            pygame.draw.rect(self.screen, (150, 50, 10), (sx, sy - 8, player.W, 8))
            pygame.draw.rect(self.screen, (150, 50, 10), (sx - 3, sy - 4, player.W + 6, 4))
            # 顔（シンプルな固定表情）
            pygame.draw.circle(self.screen, (255, 200, 150), (sx + player.W // 2, sy + 6), 7)
            # 目（固定）
            pygame.draw.circle(self.screen, BLACK, (sx + player.W // 2 - 2, sy + 4), 2)
            pygame.draw.circle(self.screen, BLACK, (sx + player.W // 2 + 4, sy + 4), 2)
            # 口（固定）
            pygame.draw.line(self.screen, BLACK, 
                           (sx + player.W // 2 - 3, sy + 10),
                           (sx + player.W // 2 + 3, sy + 10), 2)

        # 顔エンティティ描画（独立、画面固定座標）
        self._draw_face_entity()

        # HUD
        self._draw_hud()

        # カメラプレビュー
        self._draw_face_preview()

    def _draw_face_entity(self):
        """顔エンティティを画面固定座標で描画"""
        face = self.face
        fx = int(face.screen_x)
        fy = int(face.screen_y)
        size = face.size
        
        # 半透明の顔円
        face_surf = pygame.Surface((size * 2, size * 2), pygame.SRCALPHA)
        pygame.draw.circle(face_surf, (255, 220, 180, 180), (size, size), size)
        self.screen.blit(face_surf, (fx - size, fy - size))
        
        # 目
        pygame.draw.circle(self.screen, BLACK, (fx - 8, fy - 4), 3)
        pygame.draw.circle(self.screen, BLACK, (fx + 8, fy - 4), 3)
        
        # 口（開閉）
        if face.mouth_open:
            pygame.draw.ellipse(self.screen, RED, (fx - 10, fy + 4, 20, 14))
            pygame.draw.ellipse(self.screen, (100, 0, 0), (fx - 10, fy + 4, 20, 14), 2)
        else:
            pygame.draw.arc(self.screen, BLACK, (fx - 8, fy + 2, 16, 10), 0, math.pi, 2)

    def _draw_hud(self):
        player = self.player
        face   = self.face
        hud_surf = pygame.Surface((SCREEN_W, 40), pygame.SRCALPHA)
        hud_surf.fill((0, 0, 0, 140))
        self.screen.blit(hud_surf, (0, 0))

        # スコア表示（簡易）
        self._draw_simple_text(f"SC:{player.score}", 10, 12, WHITE, 2)
        self._draw_simple_text(f"CN:{player.coins_collected}", 200, 12, COIN_C, 2)
        
        # ライフアイコン
        for i in range(player.lives):
            pygame.draw.rect(self.screen, PLAYER_C, (400 + i * 22, 12, 14, 18))

        # 口パク状態インジケータ
        mouth_color = RED if face.mouth_open else (60, 60, 60)
        pygame.draw.circle(self.screen, mouth_color, (SCREEN_W - 60, 20), 12)

    def _draw_face_preview(self):
        """カメラ映像を右下に縮小表示"""
        if not self.face_tracker:
            return
        _, _, _, frame_rgb = self.face_tracker.get_state()
        if frame_rgb is None:
            return
        # numpy (H,W,3) → pygame Surface
        surf = pygame.surfarray.make_surface(np.transpose(frame_rgb, (1, 0, 2)))
        pw, ph = FACE_PREVIEW_W, FACE_PREVIEW_H
        x0 = SCREEN_W - pw - 10
        y0 = SCREEN_H - ph - 10
        pygame.draw.rect(self.screen, (40, 40, 40), (x0 - 2, y0 - 2, pw + 4, ph + 4))
        self.screen.blit(surf, (x0, y0))
        self._draw_simple_text("CAM", x0 + 4, y0 + 4, WHITE, 1)

    # ------------------------------------------------------------------
    # 死亡 / クリア / ゲームオーバー
    # ------------------------------------------------------------------
    def _update_dead(self, dt: float):
        player = self.player
        player.vy += GRAVITY
        player.y  += player.vy
        player.dead_timer -= dt
        if player.dead_timer <= 0:
            player.lives -= 1
            if player.lives <= 0:
                self.high_score = max(self.high_score, player.score)
                self.state = GameState.GAMEOVER
            else:
                self._init_game()
                self.player.lives = player.lives
                self.player.score = player.score
                self.state = GameState.PLAYING

    def _draw_dead_overlay(self):
        ov = pygame.Surface((SCREEN_W, SCREEN_H), pygame.SRCALPHA)
        ov.fill((0, 0, 0, 100))
        self.screen.blit(ov, (0, 0))
        self._draw_simple_text("GAME OVER", SCREEN_W // 2 - 120, SCREEN_H // 2 - 40, RED, 4)

    def _update_cleared(self, dt: float):
        self.goal_timer += dt
        if self.goal_timer > 4.0:
            self.high_score = max(self.high_score, self.player.score)
            self.state = GameState.TITLE

    def _draw_cleared_overlay(self):
        ov = pygame.Surface((SCREEN_W, SCREEN_H), pygame.SRCALPHA)
        ov.fill((0, 0, 0, 80))
        self.screen.blit(ov, (0, 0))
        self._draw_simple_text("CLEAR!", SCREEN_W // 2 - 80, SCREEN_H // 2 - 60, YELLOW, 4)
        self._draw_simple_text(f"SCORE:{self.player.score}", SCREEN_W // 2 - 120, SCREEN_H // 2 + 20, WHITE, 2)

    def _draw_gameover(self):
        self.screen.fill(DARK_BLUE)
        self._draw_simple_text("GAME OVER", SCREEN_W // 2 - 120, 200, RED, 4)
        self._draw_simple_text(f"SCORE:{self.high_score}", SCREEN_W // 2 - 100, 300, YELLOW, 2)
        self._draw_simple_text("SPACE TO RETRY", SCREEN_W // 2 - 120, 400, WHITE, 2)

    # ------------------------------------------------------------------
    # キーハンドラ
    # ------------------------------------------------------------------
    def _handle_key(self, key):
        if self.state == GameState.TITLE:
            if key in (pygame.K_SPACE, pygame.K_RETURN):
                self._init_game()
                self.state = GameState.PLAYING
        elif self.state == GameState.GAMEOVER:
            if key in (pygame.K_SPACE, pygame.K_RETURN):
                self.state = GameState.TITLE
        elif self.state == GameState.PLAYING:
            if key == pygame.K_ESCAPE:
                self.state = GameState.TITLE

    # ------------------------------------------------------------------
    # 終了
    # ------------------------------------------------------------------
    def _draw_simple_text(self, text: str, x: int, y: int, color: Tuple[int, int, int], scale: int = 2):
        """簡易テキスト描画（ドットマトリクス風）"""
        # 5x7 ドットフォント（簡易版、数字とアルファベット一部のみ）
        char_map = {
            'A': [[0,1,1,1,0],[1,0,0,0,1],[1,1,1,1,1],[1,0,0,0,1],[1,0,0,0,1]],
            'C': [[0,1,1,1,0],[1,0,0,0,1],[1,0,0,0,0],[1,0,0,0,1],[0,1,1,1,0]],
            'E': [[1,1,1,1,1],[1,0,0,0,0],[1,1,1,1,0],[1,0,0,0,0],[1,1,1,1,1]],
            'G': [[0,1,1,1,0],[1,0,0,0,0],[1,0,1,1,1],[1,0,0,0,1],[0,1,1,1,0]],
            'I': [[1,1,1,1,1],[0,0,1,0,0],[0,0,1,0,0],[0,0,1,0,0],[1,1,1,1,1]],
            'L': [[1,0,0,0,0],[1,0,0,0,0],[1,0,0,0,0],[1,0,0,0,0],[1,1,1,1,1]],
            'M': [[1,0,0,0,1],[1,1,0,1,1],[1,0,1,0,1],[1,0,0,0,1],[1,0,0,0,1]],
            'N': [[1,0,0,0,1],[1,1,0,0,1],[1,0,1,0,1],[1,0,0,1,1],[1,0,0,0,1]],
            'O': [[0,1,1,1,0],[1,0,0,0,1],[1,0,0,0,1],[1,0,0,0,1],[0,1,1,1,0]],
            'R': [[1,1,1,1,0],[1,0,0,0,1],[1,1,1,1,0],[1,0,1,0,0],[1,0,0,1,1]],
            'S': [[0,1,1,1,1],[1,0,0,0,0],[0,1,1,1,0],[0,0,0,0,1],[1,1,1,1,0]],
            'T': [[1,1,1,1,1],[0,0,1,0,0],[0,0,1,0,0],[0,0,1,0,0],[0,0,1,0,0]],
            'V': [[1,0,0,0,1],[1,0,0,0,1],[1,0,0,0,1],[0,1,0,1,0],[0,0,1,0,0]],
            'Y': [[1,0,0,0,1],[0,1,0,1,0],[0,0,1,0,0],[0,0,1,0,0],[0,0,1,0,0]],
            '0': [[0,1,1,1,0],[1,0,0,1,1],[1,0,1,0,1],[1,1,0,0,1],[0,1,1,1,0]],
            '1': [[0,0,1,0,0],[0,1,1,0,0],[0,0,1,0,0],[0,0,1,0,0],[0,1,1,1,0]],
            '2': [[0,1,1,1,0],[1,0,0,0,1],[0,0,1,1,0],[0,1,0,0,0],[1,1,1,1,1]],
            '3': [[1,1,1,1,0],[0,0,0,0,1],[0,1,1,1,0],[0,0,0,0,1],[1,1,1,1,0]],
            '4': [[1,0,0,1,0],[1,0,0,1,0],[1,1,1,1,1],[0,0,0,1,0],[0,0,0,1,0]],
            '5': [[1,1,1,1,1],[1,0,0,0,0],[1,1,1,1,0],[0,0,0,0,1],[1,1,1,1,0]],
            '6': [[0,1,1,1,0],[1,0,0,0,0],[1,1,1,1,0],[1,0,0,0,1],[0,1,1,1,0]],
            '7': [[1,1,1,1,1],[0,0,0,0,1],[0,0,0,1,0],[0,0,1,0,0],[0,1,0,0,0]],
            '8': [[0,1,1,1,0],[1,0,0,0,1],[0,1,1,1,0],[1,0,0,0,1],[0,1,1,1,0]],
            '9': [[0,1,1,1,0],[1,0,0,0,1],[0,1,1,1,1],[0,0,0,0,1],[0,1,1,1,0]],
            ' ': [[0,0,0,0,0],[0,0,0,0,0],[0,0,0,0,0],[0,0,0,0,0],[0,0,0,0,0]],
            ':': [[0,0,0,0,0],[0,0,1,0,0],[0,0,0,0,0],[0,0,1,0,0],[0,0,0,0,0]],
            '!': [[0,0,1,0,0],[0,0,1,0,0],[0,0,1,0,0],[0,0,0,0,0],[0,0,1,0,0]],
        }
        
        cx = x
        for char in text.upper():
            if char in char_map:
                pattern = char_map[char]
                for row_idx, row in enumerate(pattern):
                    for col_idx, pixel in enumerate(row):
                        if pixel:
                            pygame.draw.rect(self.screen, color, 
                                           (cx + col_idx * scale, y + row_idx * scale, scale, scale))
                cx += 6 * scale
            else:
                cx += 6 * scale

    def _quit(self):
        self.cwb.stop()
        if self.face_tracker:
            self.face_tracker.stop()
        pygame.quit()
        sys.exit()


# ---------------------------------------------------------------------------
# エントリーポイント
# ---------------------------------------------------------------------------
if __name__ == "__main__":
    print("Marrio - Face Control Edition")
    print(f"CanWeeb API: {CANWEEB_API}")
    print(f"Camera index: {CAMERA_INDEX}")
    game = MarrioGame()
    game.run()
