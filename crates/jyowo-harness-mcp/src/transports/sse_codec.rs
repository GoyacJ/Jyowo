use crate::McpError;

#[derive(Debug, Clone, Copy)]
pub struct SseLimits {
    pub max_line_bytes: usize,
    pub max_event_bytes: usize,
    pub max_data_bytes: usize,
}

impl Default for SseLimits {
    fn default() -> Self {
        Self {
            max_line_bytes: 64 * 1024,
            max_event_bytes: 4 * 1024 * 1024,
            max_data_bytes: 4 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
    pub id: Option<String>,
    pub retry_ms: Option<u64>,
}

pub struct SseDecoder {
    limits: SseLimits,
    pending: Vec<u8>,
    data: String,
    data_seen: bool,
    event: Option<String>,
    id: Option<String>,
    retry_ms: Option<u64>,
    event_bytes: usize,
    at_start: bool,
    pending_cr: bool,
}

impl SseDecoder {
    #[must_use]
    pub fn new(limits: SseLimits) -> Self {
        Self {
            limits,
            pending: Vec::new(),
            data: String::new(),
            data_seen: false,
            event: None,
            id: None,
            retry_ms: None,
            event_bytes: 0,
            at_start: true,
            pending_cr: false,
        }
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<SseEvent>, McpError> {
        let mut events = Vec::new();
        for &byte in bytes {
            if self.pending_cr {
                self.pending_cr = false;
                if byte == b'\n' {
                    continue;
                }
            }
            match byte {
                b'\n' => self.process_line(&mut events)?,
                b'\r' => {
                    self.process_line(&mut events)?;
                    self.pending_cr = true;
                }
                byte => {
                    self.pending.push(byte);
                    if self.pending.len() > self.limits.max_line_bytes {
                        return Err(McpError::InvalidResponse(
                            "SSE line exceeds configured limit".to_owned(),
                        ));
                    }
                }
            }
        }
        Ok(events)
    }

    pub fn finish(&mut self) -> Result<Vec<SseEvent>, McpError> {
        let mut events = Vec::new();
        self.pending_cr = false;
        if !self.pending.is_empty() {
            self.process_line(&mut events)?;
        }
        self.data.clear();
        self.data_seen = false;
        self.event = None;
        self.id = None;
        self.retry_ms = None;
        self.event_bytes = 0;
        Ok(events)
    }

    fn process_line(&mut self, events: &mut Vec<SseEvent>) -> Result<(), McpError> {
        let mut bytes = std::mem::take(&mut self.pending);
        if self.at_start {
            self.at_start = false;
            if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
                bytes.drain(..3);
            }
        }
        let line = std::str::from_utf8(&bytes).map_err(|_| {
            McpError::InvalidResponse("SSE stream contains non-UTF-8 data".to_owned())
        })?;
        self.event_bytes = self.event_bytes.saturating_add(bytes.len() + 1);
        if self.event_bytes > self.limits.max_event_bytes {
            return Err(McpError::InvalidResponse(
                "SSE event exceeds configured limit".to_owned(),
            ));
        }
        if line.is_empty() {
            self.dispatch(events);
            return Ok(());
        }
        if line.starts_with(':') {
            return Ok(());
        }
        let (field, mut value) = line.split_once(':').unwrap_or((line, ""));
        if let Some(stripped) = value.strip_prefix(' ') {
            value = stripped;
        }
        match field {
            "data" => {
                let had_data_line = self.data_seen;
                self.data_seen = true;
                let added = value.len() + usize::from(had_data_line);
                if self.data.len().saturating_add(added) > self.limits.max_data_bytes {
                    return Err(McpError::InvalidResponse(
                        "SSE data exceeds configured limit".to_owned(),
                    ));
                }
                if had_data_line {
                    self.data.push('\n');
                }
                self.data.push_str(value);
            }
            "event" => self.event = Some(value.to_owned()),
            "id" if !value.contains('\0') => self.id = Some(value.to_owned()),
            "retry" if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) => {
                self.retry_ms = value.parse().ok();
            }
            _ => {}
        }
        Ok(())
    }

    fn dispatch(&mut self, events: &mut Vec<SseEvent>) {
        if self.data_seen || self.event.is_some() || self.id.is_some() || self.retry_ms.is_some() {
            events.push(SseEvent {
                event: self.event.take(),
                data: std::mem::take(&mut self.data),
                id: self.id.take(),
                retry_ms: self.retry_ms.take(),
            });
        } else {
            self.event = None;
            self.id = None;
            self.retry_ms = None;
        }
        self.data_seen = false;
        self.event_bytes = 0;
    }
}
