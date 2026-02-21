use clap::Parser;
use midi_proxy::MidiMessageStampled;
use midir::{
    MidiInput, MidiOutput,
    os::unix::{VirtualInput, VirtualOutput},
};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Parser)]
#[command(name = "midi_proxy")]
struct Args {
    /// Virtual MIDI port name
    #[arg(long, default_value = "MIDI Proxy", env = "MIDI_PORT_NAME")]
    name: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let name = args.name;
    let midi_in_client = MidiInput::new(&format!("{} Input Client", name))?;
    let midi_out_client = MidiOutput::new(&format!("{} Output Client", name))?;

    let (tx, mut rx) = tokio::sync::broadcast::channel::<MidiMessageStampled>(16);
    let _conn_in = midi_in_client
        .create_virtual(
            &format!("{} Input Port", name),
            move |stamp, message, _| {
                let msg = MidiMessageStampled::try_from((stamp, message));
                if let Ok(msg) = msg {
                    let _ = tx.send(msg);
                };
            },
            (),
        )
        .map_err(|e| anyhow::anyhow!("failed to create MIDI Virtual Device: {}", e))?;

    let mut conn_out = midi_out_client
        .create_virtual(&format!("{} Output Port", name))
        .map_err(|e| anyhow::anyhow!("failed to create MIDI Virtual Device: {}", e))?;

    tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            println!("Echoing: {}", msg);
            if let Err(e) = conn_out.send(msg.message.as_ref()) {
                eprintln!("Failed to send MIDI message: {}", e);
            }
        }
    });

    let token = CancellationToken::new();
    token.cancelled().await;

    Ok(())
}
