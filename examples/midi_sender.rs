use std::{thread, time::Duration};

use anyhow::{Context, bail};
use clap::Parser;
use midir::{MidiOutput, MidiOutputPort};

#[derive(Debug, Parser)]
#[command(name = "midi_dummy_sender")]
#[command(about = "Send dummy MIDI messages to an output port")]
struct Args {
    #[arg(short, long)]
    list_ports: bool,

    #[arg(short, long)]
    output_port: Option<usize>,

    #[arg(short, long, default_value_t = 500)]
    interval_us: u64,

    #[arg(short, long, default_value_t = 16)]
    count: u32,

    #[arg(short = 'C', long, default_value_t = 0)]
    channel: u8,

    #[arg(short = 'V', long, default_value_t = 100)]
    velocity: u8,
}

fn pick_output_port(ports: &[MidiOutputPort], idx: Option<usize>) -> anyhow::Result<usize> {
    if ports.is_empty() {
        bail!("MIDI output port not found");
    }

    let picked = idx.unwrap_or(0);
    if picked >= ports.len() {
        bail!(
            "invalid --output-port: {} (available: 0..{})",
            picked,
            ports.len() - 1
        );
    }

    Ok(picked)
}

fn validate_channel(channel: u8) -> anyhow::Result<u8> {
    if channel > 15 {
        bail!("invalid --channel: {} (available: 0..15)", channel);
    }
    Ok(channel)
}

fn validate_velocity(velocity: u8) -> anyhow::Result<u8> {
    if velocity > 127 {
        bail!("invalid --velocity: {} (available: 0..127)", velocity);
    }
    Ok(velocity)
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let channel = validate_channel(args.channel)?;
    let velocity = validate_velocity(args.velocity)?;

    let midi_out = MidiOutput::new("midi-sender").context("failed to create MidiOutput")?;
    let out_ports = midi_out.ports();

    println!("== MIDI Output Ports ==");
    for (idx, port) in out_ports.iter().enumerate() {
        let name = midi_out
            .port_name(port)
            .with_context(|| format!("failed to get output port name at index {idx}"))?;
        println!("[{idx}] {name}");
    }

    if args.list_ports {
        return Ok(());
    }

    let out_idx = pick_output_port(&out_ports, args.output_port)?;
    let out_port = &out_ports[out_idx];
    let out_name = midi_out
        .port_name(out_port)
        .with_context(|| format!("failed to get output port name at index {out_idx}"))?;

    println!("connecting output -> [{out_idx}] {out_name}");

    let mut conn_out = midi_out
        .connect(out_port, "midi-dummy-out-connection")
        .map_err(|err| anyhow::anyhow!("failed to connect output port [{out_idx}]: {err}"))?;

    let note_on_status = 0x90 | channel;
    let note_off_status = 0x80 | channel;
    let notes = [60_u8, 62_u8, 64_u8, 65_u8, 67_u8, 69_u8, 71_u8, 72_u8];

    println!(
        "sending {} dummy MIDI notes every {}ms on channel {}",
        args.count, args.interval_us, channel
    );

    for i in 0..args.count {
        let note = notes[(i as usize) % notes.len()];

        conn_out
            .send(&[note_on_status, note, velocity])
            .with_context(|| format!("failed to send note-on at iteration {i}"))?;
        println!("note on : ch={channel} note={note} velocity={velocity}");

        thread::sleep(Duration::from_micros(args.interval_us));

        conn_out
            .send(&[note_off_status, note, 0])
            .with_context(|| format!("failed to send note-off at iteration {i}"))?;
        println!("note off: ch={channel} note={note}");
    }

    println!("done");
    Ok(())
}
