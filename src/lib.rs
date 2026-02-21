use std::{fmt::Display, time::Duration};

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

#[derive(Debug, Clone, Copy)]
pub struct MidiMessageStampled {
    pub timestamp: Duration,
    pub message: MidiMessage,
}

impl TryFrom<(u64, &[u8])> for MidiMessageStampled {
    type Error = error::Error;

    fn try_from((timestamp, bytes): (u64, &[u8])) -> Result<Self, Self::Error> {
        if bytes.len() != 3 {
            return Err(error::Error::from_midi_message(timestamp, bytes));
        }
        Ok(Self {
            timestamp: Duration::from_micros(timestamp),
            message: MidiMessage {
                status: bytes[0],
                data1: bytes[1],
                data2: bytes[2],
            },
        })
    }
}

impl Display for MidiMessageStampled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ts: {:.3}ms, [{:02X}, {:02X}, {:02X}]",
            self.timestamp.as_millis(),
            self.message.status,
            self.message.data1,
            self.message.data2
        )
    }
}
