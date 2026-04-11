/**
 * Marrio - STM32 ロータリーエンコーダ ヘッダ
 *
 * main.c の while(1) から Marrio_Encoder_Task() を呼び出してください。
 */

#ifndef MARRIO_ENCODER_H
#define MARRIO_ENCODER_H

#ifdef __cplusplus
extern "C" {
#endif

/**
 * エンコーダ読み取り + UART 送信タスク。
 * メインループから毎サイクル呼び出す。
 */
void Marrio_Encoder_Task(void);

#ifdef __cplusplus
}
#endif

#endif /* MARRIO_ENCODER_H */
