use harness_contracts::ModelError;

#[derive(Debug, Default)]
pub(crate) struct IncrementalSseDecoder {
    buffer: Vec<u8>,
}

impl IncrementalSseDecoder {
    pub(crate) fn push(&mut self, chunk: &[u8]) -> Result<Vec<String>, ModelError> {
        self.buffer.extend_from_slice(chunk);
        self.drain_complete_frames()
    }

    pub(crate) fn finish(&mut self) -> Result<Vec<String>, ModelError> {
        let mut frames = self.drain_complete_frames()?;
        if !self.buffer.iter().all(u8::is_ascii_whitespace) {
            let frame = std::mem::take(&mut self.buffer);
            frames.push(decode_frame(frame)?);
        } else {
            self.buffer.clear();
        }
        Ok(frames)
    }

    fn drain_complete_frames(&mut self) -> Result<Vec<String>, ModelError> {
        let mut frames = Vec::new();
        while let Some((end, separator_len)) = frame_end(&self.buffer) {
            let remainder = self.buffer.split_off(end + separator_len);
            let mut frame = std::mem::replace(&mut self.buffer, remainder);
            frame.truncate(end);
            frames.push(decode_frame(frame)?);
        }
        Ok(frames)
    }
}

fn decode_frame(frame: Vec<u8>) -> Result<String, ModelError> {
    String::from_utf8(frame)
        .map_err(|_| ModelError::UnexpectedResponse("invalid UTF-8 in SSE stream".to_owned()))
}

fn frame_end(buffer: &[u8]) -> Option<(usize, usize)> {
    for index in 0..buffer.len() {
        if buffer[index..].starts_with(b"\r\n\r\n") {
            return Some((index, 4));
        }
        if buffer[index..].starts_with(b"\n\n") || buffer[index..].starts_with(b"\r\r") {
            return Some((index, 2));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::IncrementalSseDecoder;

    #[test]
    fn accepts_multibyte_utf8_split_at_every_byte_boundary() {
        let stream = "data: {\"text\":\"你好𠮷\"}\r\n\r\n".as_bytes();

        for split in 1..stream.len() {
            let mut decoder = IncrementalSseDecoder::default();
            assert!(decoder.push(&stream[..split]).unwrap().is_empty());
            assert_eq!(
                decoder.push(&stream[split..]).unwrap(),
                vec!["data: {\"text\":\"你好𠮷\"}".to_owned()],
                "failed at byte boundary {split}",
            );
        }
    }

    #[test]
    fn rejects_invalid_utf8_in_a_complete_frame() {
        let mut decoder = IncrementalSseDecoder::default();
        let error = decoder.push(b"data: \xff\n\n").unwrap_err();
        assert_eq!(
            error,
            harness_contracts::ModelError::UnexpectedResponse(
                "invalid UTF-8 in SSE stream".to_owned()
            )
        );
    }

    #[test]
    fn recognizes_crlf_separator_split_across_chunks() {
        let mut decoder = IncrementalSseDecoder::default();
        assert!(decoder.push(b"data: ok\r").unwrap().is_empty());
        assert_eq!(
            decoder.push(b"\n\r\n").unwrap(),
            vec!["data: ok".to_owned()]
        );
    }
}
