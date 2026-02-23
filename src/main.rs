use std::path::PathBuf;

use bytes::Bytes;
use clap::Parser;
use futures::{SinkExt, StreamExt};
use midi_proxy::{
    ConnectionIdent, Message, MessageDetail, MidiMessage, OutputMessage, Registry, Server,
};
use tokio::{net::TcpStream, sync::mpsc, task::LocalSet};
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tracing::{error, info};

#[derive(Debug, Parser)]
#[command(name = "midi_proxy")]
struct Args {
    /// Virtual MIDI port name
    #[arg(long, default_value = "MIDI Proxy", env = "MIDI_PORT_NAME")]
    name: String,

    /// Port number to listen for MIDI messages
    #[arg(short = 'p', long, env = "MIDI_TCP_PORT")]
    port: Option<u16>,

    /// TCP接続先情報を記述したTOMLファイルへのパス
    #[arg(short, long, env = "MIDI_PROXY_CONFIG")]
    config: Option<PathBuf>,
}

/// ライブラリの初期化関数
pub fn init() {
    use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init();
    let args = Args::parse();

    let (reg, tx, rx, task) = Registry::new(args.name)?;
    let mut server = Server::new(reg, rx);

    let local_set = LocalSet::new();
    local_set.spawn_local(task);
    local_set.spawn_local(async move {
        if let Err(e) = server.run().await {
            error!("Server error: {}", e);
        }
    });

    if let Some(links) = args.config {
        let config = tokio::fs::read_to_string(links).await?;
        let config: midi_proxy::config::Config = toml::from_str(&config)?;

        for link in config.connection {
            let addr = link.addr;
            let port = link.port;
            info!("Adding TCP connection to {}:{}", addr, port);
            let ident = ConnectionIdent::Tcp(addr.parse()?);
            let framed = Framed::new(
                TcpStream::connect((addr.as_str(), port)).await?,
                LengthDelimitedCodec::new(),
            );
            let (tx_socket, rx_socket) = mpsc::channel::<OutputMessage>(16);
            tx.send((ident, MessageDetail::Connection(tx_socket)).into())
                .await?;
            // こちらから接続した場合はIn -> Outのルートを作る

            tokio::spawn(handle_connection(framed, ident, tx.clone(), rx_socket));
        }
    }

    if let Some(port) = args.port {
        let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
        info!("Listening for TCP connections on port {}", port);
        let tx = tx.clone();
        let tcp_task = async move {
            loop {
                let (socket, addr) = listener.accept().await?;
                let ident = ConnectionIdent::Tcp(addr.ip());
                info!("New TCP connection from {:?} (ident: {:?})", addr, ident);
                let framed = Framed::new(socket, LengthDelimitedCodec::new());
                let (tx_socket, rx_socket) = mpsc::channel::<OutputMessage>(16);
                tx.send((ident, MessageDetail::Connection(tx_socket)).into())
                    .await?;
                tokio::spawn(handle_connection(framed, ident, tx.clone(), rx_socket));
            }
            #[allow(unreachable_code)]
            Ok::<(), anyhow::Error>(())
        };
        local_set.spawn_local(tcp_task);
    }

    local_set.await;

    Ok(())
}

// TCPソケット単位のタスク
// socketから受信したらIdentをつけて内側へ
// 内側から来たものはsocketへ送る
// TODO: Channel書き換えフィルタがあると複数制御しやすい
async fn handle_connection(
    mut framed: Framed<TcpStream, LengthDelimitedCodec>,
    ident: ConnectionIdent,
    tcp_in: mpsc::Sender<Message>,
    mut tcp_out: mpsc::Receiver<OutputMessage>,
) -> anyhow::Result<()> {
    loop {
        tokio::select! {
            Some(result) = framed.next() => {
                let bytes = result?;
                tcp_in.send((ident, MessageDetail::MidiMessage(MidiMessage::try_from(bytes.as_ref())?)).into()).await?;
            }
            Some(msg) = tcp_out.recv() => {
                match msg {
                    OutputMessage::Midi(message) => {
                        framed.send(Bytes::from(message.as_ref().to_vec())).await?;
                    }
                    OutputMessage::Exit => {
                        info!("Connection {:?} requested exit", ident);
                        break;
                    }
                }
            }
        }
    }
    Ok(())
}
