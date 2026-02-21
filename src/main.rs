use std::path::PathBuf;

use clap::Parser;
use midi_proxy::{MidiMessage, MidiMessageStampled, handle_connection};
use midir::{
    MidiInput, MidiOutput,
    os::unix::{VirtualInput, VirtualOutput},
};
use tokio::{
    net::TcpStream,
    sync::{broadcast, mpsc},
};
use tokio_util::{
    codec::{Framed, LengthDelimitedCodec},
    sync::CancellationToken,
};
use tracing::{error, info};

#[derive(Debug, Parser)]
#[command(name = "midi_proxy")]
struct Args {
    /// Virtual MIDI port name
    #[arg(long, default_value = "MIDI Proxy", env = "MIDI_PORT_NAME")]
    name: String,

    /// Port number to listen for MIDI messages
    #[arg(short = 'p', long, env = "MIDI_TCP_PORT")]
    listen_port: Option<u16>,

    /// TCP接続先情報を記述したTOMLファイルへのパス
    #[arg(long)]
    links: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let name = args.name;
    let midi_in_client = MidiInput::new(&format!("{} Input Client", name))?;
    let midi_out_client = MidiOutput::new(&format!("{} Output Client", name))?;

    // Receive from Local MIDI Input
    let (tx_in, rx_in) = broadcast::channel::<MidiMessageStampled>(16);
    let _conn_in = midi_in_client
        .create_virtual(
            &format!("{} Input Port", name),
            move |stamp, message, _| {
                let msg = MidiMessageStampled::try_from((stamp, message));
                if let Ok(msg) = msg {
                    let _ = tx_in.send(msg);
                };
            },
            (),
        )
        .map_err(|e| anyhow::anyhow!("failed to create MIDI Virtual Device: {}", e))?;

    let (tx_out, mut rx_out) = mpsc::channel::<MidiMessage>(16);
    let mut conn_out = midi_out_client
        .create_virtual(&format!("{} Output Port", name))
        .map_err(|e| anyhow::anyhow!("failed to create MIDI Virtual Device: {}", e))?;

    // Forward to Local MIDI Output
    let mut local_input = rx_in.resubscribe();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                Ok(msg) = local_input.recv() => {
                    info!("Received MIDI message: {}", msg);
                    if let Err(e) = conn_out.send(msg.message.as_ref()) {
                        error!("Failed to send MIDI message: {}", e);
                    }
                }
                Some(msg) = rx_out.recv() => {
                    info!("Received MIDI message from TCP: {}", msg);
                    if let Err(e) = conn_out.send(msg.as_ref()) {
                        error!("Failed to send MIDI message: {}", e);
                    }
                }
            }
        }
    });

    // Listen for MIDI messages from TCP
    if let Some(port) = args.listen_port {
        info!("Listening for MIDI messages on TCP port {port}...");
        let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
            .await
            .map_err(|e| anyhow::anyhow!("failed to bind TCP listener: {}", e))?;
        let tx_out = tx_out.clone();
        let rx_in = rx_in.resubscribe();
        tokio::spawn(async move {
            loop {
                let (socket, addr) = match listener.accept().await {
                    Ok(conn) => conn,
                    Err(e) => {
                        error!("Failed to accept TCP connection: {}", e);
                        continue;
                    }
                };
                info!("Accepted TCP connection from {addr}");

                let framed = Framed::new(socket, LengthDelimitedCodec::new());
                tokio::spawn(handle_connection(
                    framed,
                    tx_out.clone(),
                    rx_in.resubscribe(),
                ));
            }
        });
    }

    // Connect to TCP servers specified in config
    if let Some(links_path) = args.links {
        let config_str = std::fs::read_to_string(&links_path)
            .map_err(|e| anyhow::anyhow!("failed to read links config file: {}", e))?;
        let config: midi_proxy::config::Config = toml::from_str(&config_str)
            .map_err(|e| anyhow::anyhow!("failed to parse links config file: {}", e))?;

        for link in config.connection {
            let target = format!("{}:{}", link.addr, link.port);
            let tx_out = tx_out.clone();
            let rx_in = rx_in.resubscribe();
            tokio::spawn(async move {
                loop {
                    println!("[Client] Attempting to connect to {}...", target);
                    match TcpStream::connect(&target).await {
                        Ok(socket) => {
                            println!("[Client] Connected to {}", target);
                            let framed = Framed::new(socket, LengthDelimitedCodec::new());
                            // 接続が切れるまで待機
                            let _ = handle_connection(framed, tx_out.clone(), rx_in.resubscribe())
                                .await;
                            println!("[Client] Connection lost. Retrying in 5s...");
                        }
                        Err(e) => {
                            println!("[Client] Connection failed: {}. Retrying in 5s...", e);
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            });
        }
    }

    let token = CancellationToken::new();
    token.cancelled().await;

    Ok(())
}
