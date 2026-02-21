pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("MIDI message parsing error: {timestamp}, {message:?}")]
    MidiMessage { timestamp: u64, message: Vec<u8> },
}

impl Error {
    pub(crate) fn from_midi_message(timestamp: u64, message: &[u8]) -> Self {
        Self::MidiMessage {
            timestamp,
            message: message.to_vec(),
        }
    }
}
