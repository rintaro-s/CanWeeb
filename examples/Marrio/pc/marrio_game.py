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
import mediapipe as mp
import numpy as np
from dataclasses import dataclass, field
from typing import List, Optional, Tuple
from enum import Enum, auto

# ---------------------------------------------------------------------------
# 設定
# ---------------------------------------------------------------------------
CANWEEB_API   = os.environ.get("CANWEEB_API", "http://localhost:8080")
CAMERA_INDEX  = int(os.environ.get("CAMERA_INDEX", "0"))
POLL_INTERVAL = 0.08          # CWB ポーリング間隔 (秒)
SCREEN_W, SCREEN_H = 1280, 720
FPS           = 60
GRAVITY       = 0.55
JUMP_VY       = -13.0
PLAYER_SPEED  = 5.0
TILE_W, TILE_H = 48, 48
FACE_PREVIEW_W, FACE_PREVIEW_H = 240, 180   # 右下に表示するカメラ縮小サイズ
MOUTH_OPEN_THRESHOLD = 0.06   # 口開閉の比率閾値

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
# CanWeeb クライアント (非同期ポーリング)
# ---------------------------------------------------------------------------
class CanWeebClient:
    def __init__(self, api_base: str):
        self.api_base = api_base.rstrip("/")
        self._session = requests.Session()
        self._lock    = threading.Lock()
        self._jump_pending  = False
        self._move_dir: Optional[str] = None  # "left" | "right" | None
        self._seen_topics: dict[str, float] = {}
        self._running = True
        self._thread  = threading.Thread(target=self._poll_loop, daemon=True)
        self._thread.start()

    def consume_jump(self) -> bool:
        with self._lock:
            v = self._jump_pending
            self._jump_pending = False
            return v

    def consume_move(self) -> Optional[str]:
        with self._lock:
            v = self._move_dir
            self._move_dir = None
            return v

    def stop(self):
        self._running = False

    def _poll_loop(self):
        while self._running:
            try:
                self._poll_topic("marrio/input/jump",  self._on_jump)
                self._poll_topic("marrio/input/move",  self._on_move)
            except Exception as e:
                pass
            time.sleep(POLL_INTERVAL)

    def _poll_topic(self, topic: str, handler):
        try:
            resp = self._session.get(
                f"{self.api_base}/api/topic",
                params={"name": topic},
                timeout=1.0,
            )
            if resp.status_code == 404:
                return
            resp.raise_for_status()
            data = resp.json()
            recv_ms = data.get("received_at_ms", 0)
            prev_ms = self._seen_topics.get(topic, 0)
            if recv_ms > prev_ms:
                self._seen_topics[topic] = recv_ms
                payload_b64 = data.get("payload_base64", "")
                if payload_b64:
                    raw = base64.b64decode(payload_b64).decode("utf-8", errors="replace")
                    handler(raw)
        except Exception:
            pass

    def _on_jump(self, raw: str):
        with self._lock:
            self._jump_pending = True

    def _on_move(self, raw: str):
        try:
            obj = json.loads(raw)
            direction = obj.get("direction", "")
            if direction in ("left", "right"):
                with self._lock:
                    self._move_dir = direction
        except Exception:
            pass


# ---------------------------------------------------------------------------
# 顔追跡 (MediaPipe FaceMesh)
# ---------------------------------------------------------------------------
class FaceTracker:
    UPPER_LIP = 13
    LOWER_LIP = 14
    NOSE_TIP  = 1
    LEFT_EYE  = 33
    RIGHT_EYE = 263

    def __init__(self, camera_index: int = 0):
        self.cap = cv2.VideoCapture(camera_index)
        self.cap.set(cv2.CAP_PROP_FRAME_WIDTH,  640)
        self.cap.set(cv2.CAP_PROP_FRAME_HEIGHT, 480)
        mp_face = mp.solutions.face_mesh
        self.mesh = mp_face.FaceMesh(
            max_num_faces=1,
            refine_landmarks=True,
            min_detection_confidence=0.5,
            min_tracking_confidence=0.5,
        )
        self._lock       = threading.Lock()
        self._face_x     = 0.5   # 0.0〜1.0 (左→右)
        self._face_y     = 0.5   # 0.0〜1.0 (上→下)
        self._mouth_open = False
        self._frame_rgb: Optional[np.ndarray] = None
        self._running = True
        self._thread  = threading.Thread(target=self._capture_loop, daemon=True)
        self._thread.start()

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

            frame = cv2.flip(frame, 1)  # 左右反転 (鏡像)
            rgb   = cv2.cvtColor(frame, cv2.COLOR_BGR2RGB)
            result = self.mesh.process(rgb)

            face_x, face_y = 0.5, 0.5
            mouth_open = False

            if result.multi_face_landmarks:
                lm = result.multi_face_landmarks[0].landmark
                nose     = lm[self.NOSE_TIP]
                upper    = lm[self.UPPER_LIP]
                lower    = lm[self.LOWER_LIP]
                left_eye = lm[self.LEFT_EYE]
                right_eye= lm[self.RIGHT_EYE]

                face_x = nose.x          # 0〜1
                face_y = nose.y          # 0〜1

                # 口の開き具合 (目間距離で正規化)
                eye_dist    = math.hypot(right_eye.x - left_eye.x,
                                         right_eye.y - left_eye.y)
                mouth_gap   = abs(lower.y - upper.y)
                if eye_dist > 1e-4:
                    ratio = mouth_gap / eye_dist
                    mouth_open = ratio > MOUTH_OPEN_THRESHOLD

            # プレビュー用縮小フレーム (RGB, small)
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
    face_x: float = 0.5   # 顔の横位置 0-1
    face_y: float = 0.5   # 顔の縦位置 0-1
    mouth_open: bool = False
    score: int = 0
    lives: int = PLAYER_LIVES
    coins_collected: int = 0
    invincible: float = 0.0  # 無敵時間(秒)
    W: int = field(default=TILE_W - 6, init=False)
    H: int = field(default=TILE_H - 4, init=False)

    def rect(self):
        return pygame.Rect(int(self.x), int(self.y), self.W, self.H)


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
        pygame.mixer.init(frequency=44100, size=-16, channels=2, buffer=512)
        self.screen = pygame.display.set_mode((SCREEN_W, SCREEN_H))
        pygame.display.set_caption("Marrio - Face Control Edition")
        self.clock  = pygame.time.Clock()

        # フォント
        self.font_big   = pygame.font.SysFont("monospace", 48, bold=True)
        self.font_mid   = pygame.font.SysFont("monospace", 28, bold=True)
        self.font_small = pygame.font.SysFont("monospace", 20)

        # CanWeeb クライアント
        self.cwb = CanWeebClient(CANWEEB_API)

        # 顔追跡
        try:
            self.face_tracker: Optional[FaceTracker] = FaceTracker(CAMERA_INDEX)
            print("[FaceTracker] カメラ初期化成功")
        except Exception as e:
            print(f"[FaceTracker] カメラ初期化失敗: {e} → マウス操作に切り替え")
            self.face_tracker = None

        # サウンド生成 (pygame.sndarray)
        self._sounds = self._create_sounds()

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
        self.camera_x = 0.0
        self.goal_reached = False
        self.goal_timer   = 0.0

    # ------------------------------------------------------------------
    # プロシージャルサウンド
    # ------------------------------------------------------------------
    def _create_sounds(self) -> dict:
        sounds = {}
        sr = 44100
        def make_tone(freq: float, duration: float, vol: float = 0.3, kind="square") -> pygame.mixer.Sound:
            n = int(sr * duration)
            t = np.linspace(0, duration, n, endpoint=False)
            if kind == "square":
                wave = vol * np.sign(np.sin(2 * np.pi * freq * t))
            elif kind == "noise":
                wave = vol * (2 * np.random.rand(n) - 1)
            else:
                wave = vol * np.sin(2 * np.pi * freq * t)
            wave = (wave * 32767).astype(np.int16)
            stereo = np.column_stack([wave, wave])
            return pygame.sndarray.make_sound(stereo)

        sounds["jump"]  = make_tone(520, 0.12, 0.25, "square")
        sounds["coin"]  = make_tone(880, 0.15, 0.2,  "sine")
        sounds["stomp"] = make_tone(200, 0.1,  0.3,  "noise")
        sounds["die"]   = make_tone(180, 0.4,  0.25, "square")
        sounds["clear"] = make_tone(660, 0.3,  0.2,  "sine")
        return sounds

    def _play(self, name: str):
        s = self._sounds.get(name)
        if s:
            s.play()

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

    def _resolve_collision(self, entity, vx: float, vy: float) -> Tuple[float, float, bool, bool]:
        """AABB タイル衝突解決。返り値: (new_vx, new_vy, on_ground, hit_ceiling)"""
        W = entity.W
        H = entity.H
        ex, ey = entity.x, entity.y
        on_ground   = False
        hit_ceiling = False

        # 横移動
        ex += vx
        left_tile  = int(ex // TILE_W)
        right_tile = int((ex + W - 1) // TILE_W)
        for ty in range(int(ey // TILE_H), int((ey + H - 1) // TILE_H) + 1):
            if vx > 0 and self._is_solid(right_tile, ty):
                ex = right_tile * TILE_W - W
                vx = 0
                break
            if vx < 0 and self._is_solid(left_tile, ty):
                ex = (left_tile + 1) * TILE_W
                vx = 0
                break

        # 縦移動
        ey += vy
        top_tile    = int(ey // TILE_H)
        bottom_tile = int((ey + H - 1) // TILE_H)
        left_tile   = int(ex // TILE_W)
        right_tile  = int((ex + W - 1) // TILE_W)

        if vy > 0:  # 落下
            for tx in range(left_tile, right_tile + 1):
                if self._is_solid(tx, bottom_tile):
                    ey = bottom_tile * TILE_H - H
                    vy = 0
                    on_ground = True
                    break
        elif vy < 0:  # 上昇
            for tx in range(left_tile, right_tile + 1):
                if self._is_solid(tx, top_tile):
                    ey = (top_tile + 1) * TILE_H
                    vy = 0
                    hit_ceiling = True
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
            self._play("coin")
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
        surf = self.font_big.render("MARRIO", True, YELLOW)
        self.screen.blit(surf, (SCREEN_W // 2 - surf.get_width() // 2, 160))
        surf2 = self.font_mid.render("Face Control Edition", True, WHITE)
        self.screen.blit(surf2, (SCREEN_W // 2 - surf2.get_width() // 2, 240))

        lines = [
            "操作方法:",
            "  顔を左右に動かす → 移動 (カメラ操作)",
            "  口を開く → アイテム取得",
            "  超音波センサに近づく → ジャンプ (RasPi-A)",
            "  ロータリーエンコーダ → 左右移動 (RasPi-B)",
            "",
            "キーボード補助: ← → 移動  Space/↑ ジャンプ",
            "",
            "   SPACE / ENTER で開始",
        ]
        for i, line in enumerate(lines):
            s = self.font_small.render(line, True, (200, 200, 200))
            self.screen.blit(s, (SCREEN_W // 2 - s.get_width() // 2, 310 + i * 28))

    # ------------------------------------------------------------------
    # ゲームアップデート
    # ------------------------------------------------------------------
    def _update_game(self, dt: float):
        player = self.player

        # ---- 入力収集 ----
        jump_cwb  = self.cwb.consume_jump()
        move_cwb  = self.cwb.consume_move()

        keys = pygame.key.get_pressed()

        # 顔追跡
        if self.face_tracker:
            fx, fy, mouth_open, _ = self.face_tracker.get_state()
            player.face_x     = fx
            player.face_y     = fy
            player.mouth_open = mouth_open
        else:
            mx, my = pygame.mouse.get_pos()
            player.face_x     = mx / SCREEN_W
            player.face_y     = my / SCREEN_H
            player.mouth_open = pygame.mouse.get_pressed()[0]

        if not player.alive:
            return

        # ---- 水平方向の移動決定 ----
        # 優先度: 顔 > CWB (ロータリーエンコーダ) > キーボード
        target_vx = 0.0

        # 顔の横位置で移動 (0.5 を中心にデッドゾーン ±0.1)
        face_dx = player.face_x - 0.5
        if abs(face_dx) > 0.08:
            target_vx = face_dx * PLAYER_SPEED * 2.5

        # CWB 移動イベントで加速/補正
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

        # ---- ジャンプ ----
        want_jump = (
            jump_cwb
            or keys[pygame.K_SPACE]
            or keys[pygame.K_UP]
        )
        if want_jump and player.on_ground:
            player.vy = JUMP_VY
            player.on_ground = False
            self._play("jump")
            self._spawn_particles(player.x + player.W / 2, player.y + player.H, WHITE, 6)

        # ---- 重力 ----
        player.vy += GRAVITY
        player.vy  = min(player.vy, 18.0)

        # ---- 衝突解決 ----
        vx, vy, on_ground, _ = self._resolve_collision(player, player.vx, player.vy)
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
            enemy.vx, enemy.vy, _, _ = self._resolve_collision(enemy, enemy.vx, enemy.vy)
            if enemy.x == old_ex and enemy.vx == 0:
                enemy.vx *= -1

            # プレイヤーとの衝突
            if player.invincible <= 0:
                erect = pygame.Rect(int(enemy.x), int(enemy.y), enemy.W if hasattr(enemy, 'W') else TILE_W - 6, TILE_H - 4)
                if player.rect().colliderect(erect):
                    # 踏みつけ判定: プレイヤーが上から来た
                    if player.vy > 0 and player.y + player.H - 8 <= enemy.y + 4:
                        enemy.stomped    = True
                        enemy.stomp_timer = 0.4
                        player.vy        = STOMP_VY
                        player.score    += ENEMY_SCORE
                        self._play("stomp")
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

        # ---- 口パクでコイン取得 ----
        if player.mouth_open:
            px_center = player.x + player.W / 2
            py_center = player.y + player.H / 2
            for coin in self.coins:
                if not coin.alive:
                    continue
                dist = math.hypot(coin.x - px_center, coin.y - py_center)
                if dist < TILE_W * 2:
                    coin.alive = False
                    player.score += COIN_SCORE
                    player.coins_collected += 1
                    self._play("coin")
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
                        self._play("coin")
                        self._spawn_particles(tx * TILE_W + TILE_W / 2, ty * TILE_H, COIN_C, 6)

        # ---- ゴールフラグ ----
        for ty in range(top_t, bot_t + 1):
            for tx in range(left_t, right_t + 1):
                if self._tile_at(tx, ty) == TileType.FLAG:
                    if not self.goal_reached:
                        self.goal_reached = True
                        self._play("clear")
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
        self._play("die")
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
                    c = self.font_small.render("?", True, BLACK)
                    self.screen.blit(c, (sx + TILE_W // 2 - c.get_width() // 2, sy + TILE_H // 2 - c.get_height() // 2))
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

        # プレイヤー描画
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
            # 顔
            pygame.draw.circle(self.screen, (255, 200, 150), (sx + player.W // 2, sy + 6), 7)
            if player.mouth_open:
                pygame.draw.arc(self.screen, RED,
                                (sx + player.W // 2 - 5, sy + 6, 10, 8),
                                math.pi, 2 * math.pi, 2)
            # 顔の位置に合わせて目の方向
            eye_ox = int((player.face_x - 0.5) * 4)
            pygame.draw.circle(self.screen, BLACK, (sx + player.W // 2 - 2 + eye_ox, sy + 4), 2)
            pygame.draw.circle(self.screen, BLACK, (sx + player.W // 2 + 4 + eye_ox, sy + 4), 2)

        # HUD
        self._draw_hud()

        # カメラプレビュー
        self._draw_face_preview()

    def _draw_hud(self):
        player = self.player
        hud_surf = pygame.Surface((SCREEN_W, 40), pygame.SRCALPHA)
        hud_surf.fill((0, 0, 0, 140))
        self.screen.blit(hud_surf, (0, 0))

        score_s = self.font_small.render(f"SCORE {player.score:07d}", True, WHITE)
        coins_s = self.font_small.render(f"COINS {player.coins_collected:03d}", True, COIN_C)
        lives_s = self.font_small.render(f"x{player.lives}", True, WHITE)
        self.screen.blit(score_s, (10, 10))
        self.screen.blit(coins_s, (250, 10))
        # ライフアイコン
        for i in range(player.lives):
            pygame.draw.rect(self.screen, PLAYER_C, (430 + i * 22, 12, 14, 18))
        self.screen.blit(lives_s, (430 + player.lives * 22 + 4, 10))

        # 口パク状態インジケータ
        mouth_color = RED if player.mouth_open else (60, 60, 60)
        pygame.draw.circle(self.screen, mouth_color, (SCREEN_W - 60, 20), 12)
        label = self.font_small.render("MOUTH", True, WHITE)
        self.screen.blit(label, (SCREEN_W - 110, 30))

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
        label = self.font_small.render("CAM", True, WHITE)
        self.screen.blit(label, (x0 + 4, y0 + 4))

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
        s = self.font_big.render("GAME OVER", True, RED)
        self.screen.blit(s, (SCREEN_W // 2 - s.get_width() // 2, SCREEN_H // 2 - 40))

    def _update_cleared(self, dt: float):
        self.goal_timer += dt
        if self.goal_timer > 4.0:
            self.high_score = max(self.high_score, self.player.score)
            self.state = GameState.TITLE

    def _draw_cleared_overlay(self):
        ov = pygame.Surface((SCREEN_W, SCREEN_H), pygame.SRCALPHA)
        ov.fill((0, 0, 0, 80))
        self.screen.blit(ov, (0, 0))
        s = self.font_big.render("COURSE CLEAR!", True, YELLOW)
        self.screen.blit(s, (SCREEN_W // 2 - s.get_width() // 2, SCREEN_H // 2 - 60))
        s2 = self.font_mid.render(f"SCORE: {self.player.score}", True, WHITE)
        self.screen.blit(s2, (SCREEN_W // 2 - s2.get_width() // 2, SCREEN_H // 2 + 20))

    def _draw_gameover(self):
        self.screen.fill(DARK_BLUE)
        s = self.font_big.render("GAME OVER", True, RED)
        self.screen.blit(s, (SCREEN_W // 2 - s.get_width() // 2, 200))
        s2 = self.font_mid.render(f"HIGH SCORE: {self.high_score}", True, YELLOW)
        self.screen.blit(s2, (SCREEN_W // 2 - s2.get_width() // 2, 300))
        s3 = self.font_small.render("SPACE / ENTER でタイトルへ", True, WHITE)
        self.screen.blit(s3, (SCREEN_W // 2 - s3.get_width() // 2, 400))

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
