use serde::de::DeserializeOwned;
use serde::Serialize;

use super::{IpcError, MAX_FRAME_BYTES};

pub fn encode_frame<T: Serialize>(value: &T) -> Result<Vec<u8>, IpcError> {
    let body = serde_json::to_vec(value)?;
    if body.is_empty() {
        return Err(IpcError::ZeroLengthFrame);
    }
    if body.len() > MAX_FRAME_BYTES {
        return Err(IpcError::FrameTooLarge);
    }
    let length = u32::try_from(body.len()).map_err(|_| IpcError::FrameTooLarge)?;
    let mut frame = Vec::with_capacity(4 + body.len());
    frame.extend_from_slice(&length.to_be_bytes());
    frame.extend_from_slice(&body);
    Ok(frame)
}

#[derive(Debug, Default)]
pub struct JsonFrameDecoder {
    buffered: Vec<u8>,
}

impl JsonFrameDecoder {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            buffered: Vec::new(),
        }
    }

    pub fn push<T: DeserializeOwned>(&mut self, chunk: &[u8]) -> Result<Vec<T>, IpcError> {
        if chunk.len() > MAX_FRAME_BYTES + 4 {
            return Err(IpcError::FrameTooLarge);
        }
        self.buffered.extend_from_slice(chunk);
        let mut decoded = Vec::new();
        loop {
            if self.buffered.len() < 4 {
                break;
            }
            let length = u32::from_be_bytes(
                self.buffered[..4]
                    .try_into()
                    .expect("four-byte prefix checked"),
            ) as usize;
            if length == 0 {
                return Err(IpcError::ZeroLengthFrame);
            }
            if length > MAX_FRAME_BYTES {
                return Err(IpcError::FrameTooLarge);
            }
            if self.buffered.len() < 4 + length {
                break;
            }
            let value = serde_json::from_slice(&self.buffered[4..4 + length])?;
            self.buffered.drain(..4 + length);
            decoded.push(value);
        }
        Ok(decoded)
    }
}
