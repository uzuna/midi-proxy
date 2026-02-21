# midi-proxy

MIDI メッセージを中継・検証するための Rust プロジェクトです。

リモートのMIDIデバイスを手元のMIDIデバイスで制御したいことはしばしばあります。
qmidinetはありますがUDPMulticast用途のみで、リモート操作ではSSHトンネルを使う接続や複数デバイスをつなぐなどの柔軟な運用がしたいことがあります。

デバイス上では固定のTCPサーバーとMIDIインターフェースとして見え、動的に接続先を変更できるProxy機能を提供します

## 現在の実装

- `src/main.rs`: 仮想MIDI入力を受け取り、仮想MIDI出力へ転送するシンプルなエコー処理
- `src/lib.rs`: `MidiMessage` / `MidiMessageStampled` の定義とパース処理
- `examples/`: 送信・監視を検証するためのサンプルを提供

`MidiMessageStampled::try_from((timestamp, bytes))` は現在 **3バイト固定のMIDIメッセージ** を対象にしています。

## 実行

```bash
cargo run
```

終了するには `Ctrl+C` を押します。

## Example: MIDI Sender

`examples/midi_sender.rs` は、指定した出力ポートにダミーのノートオン/ノートオフを一定間隔で送信するサンプルです。

### 使い方

1. 出力ポートを確認

```bash
cargo run --example midi_sender -- --list-ports
```

2. ダミーデータを送信（例: 1番ポートへ16回、5µs間隔）

```bash
cargo run --example midi_sender -- --output-port 1 --count 16 --interval-us 5
```

### オプション

- `--list-ports`: 出力ポート一覧を表示して終了
- `--output-port <index>`: 出力ポート番号（省略時は `0`）
- `--interval-us <us>`: 送信間隔マイクロ秒（省略時は `500`）
- `--count <n>`: 送信回数（省略時は `16`）
- `--channel <0-15>`: MIDIチャンネル（省略時は `0`）
- `--velocity <0-127>`: ノートオン時のベロシティ（省略時は `100`）

## Example: MIDI Listener

`examples/midi_listener.rs` は、指定した入力ポートを監視し、受信したMIDIメッセージを表示するサンプルです。

### 使い方

1. 入力ポートを確認

```bash
cargo run --example midi_listener -- --list-ports
```

2. 監視するポートを指定して開始（例: 1番ポート）

```bash
cargo run --example midi_listener -- --input-port 1
```

開始後は Enter キーで終了します。

### オプション

- `--list-ports`: 入力ポート一覧を表示して終了
- `--input-port <index>`: 監視する入力ポート番号（省略時は `0`）
