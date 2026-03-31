# 親 / 子 実行制御

CmdLib では、親が関数内で子向け処理を定義し、子へ送って実行できます。

## 実装済み API

- `define_child_program!`
- `send_child_program!`
- `run_child_program!`

## 使い方

1. 親で `define_child_program!` で処理を定義
2. `send_child_program!` で子へ送信
3. `ChildProgramReport` で実行結果確認
