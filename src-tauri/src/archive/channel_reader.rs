use std::io::{self, Cursor, Read};
use std::sync::mpsc::{Receiver, SyncSender};

pub(crate) enum StreamMessage {
    Data(Vec<u8>),
    Error(String),
}

/// A bounded producer/consumer bridge used by archive APIs whose entry readers
/// borrow the archive decoder. The producer blocks after two chunks, so large
/// entries are never accumulated in memory.
pub(crate) struct ChannelReader {
    receiver: Receiver<StreamMessage>,
    current: Cursor<Vec<u8>>,
    finished: bool,
}

impl ChannelReader {
    pub(crate) fn new(receiver: Receiver<StreamMessage>) -> Self {
        Self {
            receiver,
            current: Cursor::new(Vec::new()),
            finished: false,
        }
    }
}

impl Read for ChannelReader {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        loop {
            let count = self.current.read(output)?;
            if count > 0 {
                return Ok(count);
            }
            if self.finished {
                return Ok(0);
            }
            match self.receiver.recv() {
                Ok(StreamMessage::Data(chunk)) => self.current = Cursor::new(chunk),
                Ok(StreamMessage::Error(message)) => {
                    self.finished = true;
                    return Err(io::Error::new(io::ErrorKind::InvalidData, message));
                }
                Err(_) => {
                    self.finished = true;
                    return Ok(0);
                }
            }
        }
    }
}

pub(crate) fn copy_to_channel(
    reader: &mut dyn Read,
    sender: &SyncSender<StreamMessage>,
) -> io::Result<()> {
    let mut buffer = vec![0; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            return Ok(());
        }
        if sender
            .send(StreamMessage::Data(buffer[..count].to_vec()))
            .is_err()
        {
            return Ok(());
        }
    }
}

pub(crate) fn send_error(sender: &SyncSender<StreamMessage>, error: impl ToString) {
    let _ = sender.send(StreamMessage::Error(error.to_string()));
}
