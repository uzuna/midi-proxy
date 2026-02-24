use std::{fmt::Display, net::IpAddr, sync::Arc, time::Duration};

use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use midir::{MidiInput, MidiInputConnection, MidiOutput};
use tokio::{
    net::TcpStream,
    sync::{broadcast, mpsc},
};
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tracing::{error, info};

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

/// ルーティングの管理を行う構造体
pub struct Registry {
    // 出力へのチャンネル保持。Routerでは町が発生しないように同期で送信する形とする
    outputs: rustc_hash::FxHashMap<ConnectionIdent, mpsc::Sender<OutputMessage>>,
    // ルーティングテーブル。入力識別子から出力識別子のリストへのマッピング
    route_table: rustc_hash::FxHashMap<ConnectionIdent, Vec<ConnectionIdent>>,
    // MIDI入力の接続情報。Drop回避のために保持を行う
    _midi_input: Option<MidiInputConnection<()>>,
    // 自身のMIDI Outに必ず送るために覚えておく
    local_midi_out: ConnectionIdent,
}

impl Registry {
    #[cfg(unix)]
    pub fn new(
        name: impl AsRef<str>,
    ) -> anyhow::Result<(
        Self,
        mpsc::Sender<Message>,
        mpsc::Receiver<Message>,
        impl std::future::Future<Output = anyhow::Result<()>>,
    )> {
        use midir::os::unix::{VirtualInput, VirtualOutput};
        let (tx, rx) = mpsc::channel::<Message>(16);
        let name = name.as_ref();
        let midi_in = MidiInput::new(&format!("{} Input", name))?;
        let midi_out = MidiOutput::new(&format!("{} Output", name))?;

        for x in midi_in.ports() {
            info!("Existing MIDI input port: {}", midi_in.port_name(&x)?);
        }

        // MIDI仮想デバイスを作成して登録
        let count = midi_in.port_count(); // MIDIデバイスの初期化のために必要
        let ident = ConnectionIdent::Midi(count as u8); // 仮に現在のポート数を識別子として使用
        info!(
            "Creating MIDI virtual device with ident {:?} ({} existing ports)",
            ident, count,
        );
        let port_name = format!("{} Input Port", name);
        let tx_clone = tx.clone();
        let ident_clone = ident.clone();
        let conn_in = midi_in
            .create_virtual(
                &port_name,
                move |stamp, message, _| {
                    let msg = MidiMessageStampled::try_from((stamp, message));
                    match msg {
                        Ok(msg) => {
                            use tracing::trace;
                            trace!(
                                "Received MIDI message: timestamp={}, message={:?}",
                                stamp, message
                            );
                            let _ = tx_clone.blocking_send(Message::MidiMessage((
                                ident_clone.clone(),
                                msg.message,
                            )));
                        }
                        Err(e) => {
                            let _ = tx_clone
                                .blocking_send(Message::Error((ident_clone.clone(), e.into())));
                        }
                    }
                },
                (),
            )
            .map_err(|e| anyhow::anyhow!("failed to create MIDI Virtual Device: {}", e))?;

        // crate Output構造体を作成して登録
        let mut conn_out = midi_out
            .create_virtual(&format!("{} Output Port", name))
            .map_err(|e| anyhow::anyhow!("failed to create MIDI Virtual Device: {}", e))?;
        let (tx_midi_out, mut rx_midi_out) = mpsc::channel::<OutputMessage>(16);
        let ident_clone = ident.clone();
        let task = async move {
            use tracing::info;

            while let Some(msg) = rx_midi_out.recv().await {
                match msg {
                    OutputMessage::Midi(midi) => conn_out.send(midi.as_ref())?,
                    OutputMessage::Exit => {
                        break;
                    }
                }
            }
            info!("MIDI Output task for {:?} is exiting", ident_clone);
            Ok(())
        };
        let outputs = [(ident.clone(), tx_midi_out)].into_iter().collect();

        // MIDIIn -> MidiOutは既定で入れる
        let route_table = [(ident.clone(), vec![ident.clone()])].into_iter().collect();
        Ok((
            Self {
                outputs,
                route_table,
                _midi_input: Some(conn_in),
                local_midi_out: ident.clone(),
            },
            tx,
            rx,
            task,
        ))
    }

    fn add_connection(&mut self, ident: ConnectionIdent, tx: mpsc::Sender<OutputMessage>) {
        info!("Added connection {:?}", ident);
        self.outputs.insert(ident.clone(), tx);
    }

    /// ローカルMIDI出力の識別子を取得する
    pub fn get_local_midi_out_ident(&self) -> &ConnectionIdent {
        &self.local_midi_out
    }
}

/// Inputを識別するためのトークン
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ConnectionIdent {
    Midi(u8),
    Tcp((Arc<String>, IpAddr, u16)),
}

/// 出力構造体
///
/// 入力はそれぞれのタスクで実行され、コールバックからはこれらの構造体を通じてMIDIメッセージを送信する。
pub struct Server {
    registry: Registry,
    rx: mpsc::Receiver<Message>,
}

impl Server {
    pub fn new(registry: Registry, rx: mpsc::Receiver<Message>) -> Self {
        Self { registry, rx }
    }

    pub async fn run(&mut self) -> anyhow::Result<()> {
        loop {
            if let Some(msg) = self.rx.recv().await {
                match msg {
                    Message::MidiMessage((ident, midi_msg)) => {
                        let r = &self.registry;
                        if let Some(outputs) = r.route_table.get(&ident) {
                            for output_ident in outputs {
                                if let Some(output) = r.outputs.get(output_ident)
                                    && let Err(e) = output.try_send(midi_msg.into())
                                {
                                    error!("Failed to send MIDI message: {}", e);
                                }
                            }
                        }
                    }
                    Message::Connection((ident, tx)) => {
                        self.registry.add_connection(ident, tx);
                    }
                    Message::Disconnection(ident) => {
                        // 切断の処理（必要に応じてルーティングを更新）
                        self.registry.outputs.remove(&ident);
                        self.registry.route_table.remove(&ident);
                        self.registry
                            .route_table
                            .values_mut()
                            .for_each(|routes| routes.retain(|x| x != &ident));
                        info!("Removed connection {:?}", ident);
                    }
                    Message::Error((ident, e)) => {
                        error!("Error from connection {:?}: {}", ident, e);
                    }
                    Message::Link((from, to)) => {
                        // ルーティングの更新
                        self.registry.route_table.entry(from).or_default().push(to);
                    }
                }
            }
        }
    }
}

pub enum Message {
    // 接続識別子と送信チャネル
    Connection((ConnectionIdent, mpsc::Sender<OutputMessage>)),
    // 転送元と転送先
    Link((ConnectionIdent, ConnectionIdent)),
    // 切断
    Disconnection(ConnectionIdent),
    // MIDIメッセージ
    MidiMessage((ConnectionIdent, MidiMessage)),
    // エラー
    Error((ConnectionIdent, anyhow::Error)),
}

pub enum Input {
    /// MIDI仮想デバイスへの入力
    MidiVirtual(MidiInputConnection<()>),
}

/// Output制御メッセージ
pub enum OutputMessage {
    Midi(MidiMessage),
    Exit,
}

impl From<MidiMessage> for OutputMessage {
    fn from(midi: MidiMessage) -> Self {
        Self::Midi(midi)
    }
}
