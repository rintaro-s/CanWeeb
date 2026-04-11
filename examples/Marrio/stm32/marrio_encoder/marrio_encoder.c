/**
 * Marrio - STM32 ロータリーエンコーダ
 *
 * 対象: STM32F103C8T6 (Blue Pill) / STM32F4xx など HAL 対応ボード
 * ツール: STM32CubeIDE / STM32CubeMX + HAL ライブラリ
 *
 * 配線:
 *   エンコーダ A 相 → PB0 (D3)
 *   エンコーダ B 相 → PB7 (D4)
 *   GND → GND
 *   VCC → 3.3V
 *
 * UART 出力 (USART1, 115200 baud, TX=PA9):
 *   右回転: "R\r\n"
 *   左回転: "L\r\n"
 *   ※ 変化がある場合のみ送信
 *
 * CubeMX 設定:
 *   - PB0: GPIO_Input, Pull-Up
 *   - PB7: GPIO_Input, Pull-Up
 *   - USART1: Asynchronous, 115200, 8N1
 *   - TIM2 または SysTick: 1ms tick
 */

#include "main.h"
#include <string.h>

/* ユーザー定義 --------------------------------------------------------- */
#define ENC_PIN_A_PORT  GPIOB
#define ENC_PIN_A_PIN   GPIO_PIN_0   /* PB0 = D3 */
#define ENC_PIN_B_PORT  GPIOB
#define ENC_PIN_B_PIN   GPIO_PIN_7   /* PB7 = D4 */

#define UART_HANDLE     huart1
#define UART_TX_TIMEOUT 10           /* ms */

/* デバウンス: 同一方向の連続送信を抑制する最小間隔 (ms) */
#define SEND_COOLDOWN_MS 30
/* -------------------------------------------------------------------- */

extern UART_HandleTypeDef UART_HANDLE;

/* 内部状態 */
static uint8_t  s_prev_state = 0xFF;  /* 初回は必ず更新させる */
static uint32_t s_last_send_ms = 0;

/* 前方宣言 */
static uint8_t  read_encoder_state(void);
static int8_t   decode_quadrature(uint8_t prev, uint8_t curr);
static void     uart_send_str(const char *str);

/**
 * メインループから呼び出す処理。
 * main() の while(1) 内に配置してください。
 */
void Marrio_Encoder_Task(void)
{
    uint8_t curr_state = read_encoder_state();

    if (curr_state == s_prev_state) {
        return;  /* 変化なし */
    }

    int8_t delta = decode_quadrature(s_prev_state, curr_state);
    s_prev_state = curr_state;

    if (delta == 0) {
        return;  /* グリッチ */
    }

    uint32_t now = HAL_GetTick();
    if ((now - s_last_send_ms) < SEND_COOLDOWN_MS) {
        return;  /* クールダウン中 */
    }
    s_last_send_ms = now;

    if (delta > 0) {
        uart_send_str("R\r\n");
    } else {
        uart_send_str("L\r\n");
    }
}

/**
 * PB0(A相) と PB7(B相) を読んで 2 ビット状態を返す。
 *   bit1 = A相, bit0 = B相
 */
static uint8_t read_encoder_state(void)
{
    uint8_t a = (HAL_GPIO_ReadPin(ENC_PIN_A_PORT, ENC_PIN_A_PIN) == GPIO_PIN_SET) ? 1u : 0u;
    uint8_t b = (HAL_GPIO_ReadPin(ENC_PIN_B_PORT, ENC_PIN_B_PIN) == GPIO_PIN_SET) ? 1u : 0u;
    return (uint8_t)((a << 1u) | b);
}

/**
 * グレイコード遷移から回転方向を判定する。
 * @return  +1: 右回転, -1: 左回転, 0: 無効遷移
 */
static int8_t decode_quadrature(uint8_t prev, uint8_t curr)
{
    /* (prev << 2) | curr の 4 ビットで方向が決まる */
    uint8_t key = (uint8_t)(((prev & 0x03u) << 2u) | (curr & 0x03u));
    switch (key) {
        /* 正転: 00→01→11→10→00 */
        case 0x01u: /* 00→01 */
        case 0x07u: /* 01→11 */
        case 0x0Eu: /* 11→10 */
        case 0x08u: /* 10→00 */
            return +1;
        /* 逆転: 00→10→11→01→00 */
        case 0x02u: /* 00→10 */
        case 0x0Bu: /* 10→11 */
        case 0x0Du: /* 11→01 */
        case 0x04u: /* 01→00 */
            return -1;
        default:
            return 0;
    }
}

/**
 * UART で文字列を送信する。
 */
static void uart_send_str(const char *str)
{
    uint16_t len = (uint16_t)strlen(str);
    HAL_UART_Transmit(&UART_HANDLE, (const uint8_t *)str, len, UART_TX_TIMEOUT);
}
