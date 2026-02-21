use std::{fmt::Display, time::Duration};

use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use tokio::{
    net::TcpStream,
    sync::{broadcast, mpsc},
};
use tokio_util::codec::{Framed, LengthDelimitedCodec};

pub mod config;
pub mod error;

/// MIDIメッセージを表す構造体
#[derive(Debug, Clone, Copy)]
pub struct MidiMessage {
    pub status: u8,
    pub data1: u8,
    pub data2: u8,
}

impl AsRef<[u8]> for MidiMessage {
    fn as_ref(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self as *const MidiMessage as *const u8, 3) }
    }
}

impl TryFrom<&[u8]> for MidiMessage {
    type Error = error::Error;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        if bytes.len() != 3 {
            return Err(error::Error::from_midi_message(0, bytes));
        }
        Ok(Self {
            status: bytes[0],
            data1: bytes[1],
            data2: bytes[2],
        })
    }
}

impl Display for MidiMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{:02X}, {:02X}, {:02X}]",
            self.status, self.data1, self.data2
        )
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MidiMessageStampled {
    pub timestamp: Duration,
    pub message: MidiMessage,
}

impl TryFrom<(u64, &[u8])> for MidiMessageStampled {
    type Error = error::Error;

    fn try_from((timestamp, bytes): (u64, &[u8])) -> Result<Self, Self::Error> {
        let message = MidiMessage::try_from(bytes)?;
        Ok(Self {
            timestamp: Duration::from_micros(timestamp),
            message,
        })
    }
}

impl Display for MidiMessageStampled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ts: {:.3}ms, {}",
            self.timestamp.as_millis(),
            self.message
        )
    }
}

/// 接続（サーバー/クライアント共通）のMIDI送受信処理
pub async fn handle_connection(
    // 受信データ
    mut framed: Framed<TcpStream, LengthDelimitedCodec>,
    // ローカルMIDI入力からのメッセージを受け取るためのbroadcastチャネル
    tx_out: mpsc::Sender<MidiMessage>,
    // ローカルMIDI入力からのメッセージを受け取るためのbroadcastチャネルのReceiver
    mut rx_in: broadcast::Receiver<MidiMessageStampled>,
) -> anyhow::Result<()> {
    loop {
        tokio::select! {
        // 1. ネットワーク経由で受信 -> ローカルMIDI出力 & 他へ転送はしない(ループ防止)
        Some(result) = framed.next() => {
            match result {
                Ok(bytes) => {
                    tx_out.send(MidiMessage::try_from(bytes.as_ref())?).await?;
                }
                Err(e) => return Err(anyhow::anyhow!("Network error: {}", e)),
            }
        }
        // 2. ローカルMIDI入力(broadcast) -> ネットワークへ送信
        Ok(msg) = rx_in.recv() =>
            if let Err(e) = framed.send(Bytes::from(msg.message.as_ref().to_vec())).await {
                return Err(anyhow::anyhow!("Send error: {}", e));
            }
        }
    }
}
