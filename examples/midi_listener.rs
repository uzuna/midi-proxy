use anyhow::{Context, bail};
use clap::Parser;
use midir::{MidiInput, MidiInputPort};

#[derive(Debug, Parser)]
#[command(name = "midi_listener")]
#[command(about = "Listen to a MIDI input port and print incoming messages")]
struct Args {
    #[arg(short, long)]
    list_ports: bool,

    #[arg(short, long)]
    input_port: Option<usize>,
}

fn pick_input_port(ports: &[MidiInputPort], idx: Option<usize>) -> anyhow::Result<usize> {
    if ports.is_empty() {
        bail!("MIDI input port not found");
    }

    let picked = idx.unwrap_or(0);
    if picked >= ports.len() {
        bail!(
            "invalid --input-port: {} (available: 0..{})",
            picked,
            ports.len() - 1
        );
    }

    Ok(picked)
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let midi_in = MidiInput::new("midi-listener-input").context("failed to create MidiInput")?;
    let in_ports = midi_in.ports();

    println!("== MIDI Input Ports ==");
    for (idx, port) in in_ports.iter().enumerate() {
        let name = midi_in
            .port_name(port)
            .with_context(|| format!("failed to get input port name at index {idx}"))?;
        println!("[{idx}] {name}");
    }

    if args.list_ports {
        return Ok(());
    }

    let in_idx = pick_input_port(&in_ports, args.input_port)?;
    let in_port = &in_ports[in_idx];
    let in_name = midi_in
        .port_name(in_port)
        .with_context(|| format!("failed to get input port name at index {in_idx}"))?;

    println!("listening <- [{in_idx}] {in_name}");

    let _in_conn = midi_in
        .connect(
            in_port,
            "midi-listener-in-connection",
            move |stamp, message, _| {
                let msg = midi_proxy::MidiMessageStampled::try_from((stamp, message));
                if let Ok(msg) = msg {
                    println!("Received: {}", msg);
                } else {
                    eprintln!(
                        "Failed to parse MIDI message: timestamp={}, message={:?}",
                        stamp, message
                    );
                }
            },
            (),
        )
        .map_err(|err| anyhow::anyhow!("failed to connect input port [{in_idx}]: {err}"))?;

    println!("listener started. press Enter to stop.");
    let mut buffer = String::new();
    let _ = std::io::stdin().read_line(&mut buffer);

    Ok(())
}
