use std::io::{self, Read, Write};

use crate::MAX_FRAME_BYTES;

pub fn write_frame<T: serde::Serialize>(writer: &mut impl Write, value: &T) -> io::Result<()> {
    let payload = serde_json::to_vec(value)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if payload.is_empty() || payload.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "IPC frame exceeds the configured size limit",
        ));
    }
    let length = u32::try_from(payload.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "IPC frame is too large"))?;
    writer.write_all(&length.to_le_bytes())?;
    writer.write_all(&payload)?;
    writer.flush()
}

pub fn read_frame<T: serde::de::DeserializeOwned>(reader: &mut impl Read) -> io::Result<T> {
    let mut length = [0; 4];
    reader.read_exact(&mut length)?;
    let length = usize::try_from(u32::from_le_bytes(length))
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid IPC frame length"))?;
    if length == 0 || length > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid IPC frame length",
        ));
    }
    let mut payload = vec![0; length];
    reader.read_exact(&mut payload)?;
    serde_json::from_slice(&payload)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

#[derive(Default)]
pub struct FrameDecoder {
    header: [u8; 4],
    header_len: usize,
    body: Vec<u8>,
    body_len: usize,
}

impl FrameDecoder {
    pub fn read_available(&mut self, reader: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
        loop {
            if self.header_len < self.header.len() {
                match reader.read(&mut self.header[self.header_len..]) {
                    Ok(0) => return Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
                    Ok(read) => self.header_len += read,
                    Err(error) if is_timeout(&error) => return Ok(None),
                    Err(error) => return Err(error),
                }
                if self.header_len < self.header.len() {
                    continue;
                }
                self.body_len = usize::try_from(u32::from_le_bytes(self.header)).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "invalid IPC frame length")
                })?;
                if self.body_len == 0 || self.body_len > MAX_FRAME_BYTES {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid IPC frame length",
                    ));
                }
                self.body.resize(self.body_len, 0);
            }

            let received = self.header_len.saturating_sub(self.header.len());
            if received < self.body_len {
                let offset = self.body.len() - (self.body_len - received);
                match reader.read(&mut self.body[offset..]) {
                    Ok(0) => return Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
                    Ok(read) => self.header_len += read,
                    Err(error) if is_timeout(&error) => return Ok(None),
                    Err(error) => return Err(error),
                }
                continue;
            }

            let payload = std::mem::take(&mut self.body);
            *self = Self::default();
            return Ok(Some(payload));
        }
    }
}

fn is_timeout(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use serde::{Deserialize, Serialize};

    use super::{FrameDecoder, read_frame, write_frame};
    use crate::MAX_FRAME_BYTES;

    #[derive(Debug, Deserialize, PartialEq, Serialize)]
    struct TestMessage {
        id: u64,
        path: String,
    }

    #[test]
    fn framed_json_round_trips() {
        let message = TestMessage {
            id: 42,
            path: "notes with spaces/naïve.txt".into(),
        };
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &message).expect("frame should serialize");
        let decoded: TestMessage = read_frame(&mut Cursor::new(bytes)).expect("frame should parse");
        assert_eq!(decoded, message);
    }

    #[test]
    fn rejects_oversized_frame_before_reading_payload() {
        let length = u32::try_from(MAX_FRAME_BYTES + 1).expect("test limit fits in u32");
        let error = read_frame::<TestMessage>(&mut Cursor::new(length.to_le_bytes()))
            .expect_err("oversized frames must be rejected");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn incremental_decoder_accepts_a_complete_frame() {
        let message = TestMessage {
            id: 7,
            path: "file.txt".into(),
        };
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &message).expect("frame should serialize");
        let mut decoder = FrameDecoder::default();
        let payload = decoder
            .read_available(&mut Cursor::new(bytes))
            .expect("frame should decode")
            .expect("complete frame should be returned");
        let decoded = serde_json::from_slice::<TestMessage>(&payload).expect("json should parse");
        assert_eq!(decoded, message);
    }
}
