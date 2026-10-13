#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub event: String,
    pub data: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("SSE frame exceeded the configured byte limit")]
    TooLarge,
    #[error("SSE contained invalid UTF-8")]
    InvalidUtf8,
    #[error("SSE ended inside an event")]
    Truncated,
}

/// Bounded, incremental framing. A network chunk can split even a UTF-8 code point.
/// This parser intentionally rejects invalid UTF-8 rather than changing model output.
pub struct Decoder {
    max_frame_bytes: usize,
    frame_bytes: usize,
    line: Vec<u8>,
    data: String,
    event: String,
    has_data: bool,
    first_line: bool,
    skip_lf: bool,
}

impl Decoder {
    pub fn new(max_frame_bytes: usize) -> Self {
        Self {
            max_frame_bytes,
            frame_bytes: 0,
            line: Vec::new(),
            data: String::new(),
            event: String::new(),
            has_data: false,
            first_line: true,
            skip_lf: false,
        }
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Event>, Error> {
        let mut events = Vec::new();
        for &byte in bytes {
            if self.skip_lf {
                self.skip_lf = false;
                if byte == b'\n' {
                    continue;
                }
            }
            self.frame_bytes += 1;
            if self.frame_bytes > self.max_frame_bytes {
                return Err(Error::TooLarge);
            }
            if byte == b'\r' || byte == b'\n' {
                self.complete_line(&mut events)?;
                self.skip_lf = byte == b'\r';
            } else {
                self.line.push(byte);
            }
        }
        Ok(events)
    }

    fn complete_line(&mut self, events: &mut Vec<Event>) -> Result<(), Error> {
        let raw = std::str::from_utf8(&self.line).map_err(|_| Error::InvalidUtf8)?;
        let line = if self.first_line {
            raw.strip_prefix('\u{feff}').unwrap_or(raw)
        } else {
            raw
        };
        self.first_line = false;
        if line.is_empty() {
            if self.has_data {
                self.data.pop(); // Each data field contributed exactly one newline.
                events.push(Event {
                    event: if self.event.is_empty() {
                        "message".into()
                    } else {
                        std::mem::take(&mut self.event)
                    },
                    data: std::mem::take(&mut self.data),
                });
            }
            self.event.clear();
            self.has_data = false;
            self.frame_bytes = 0;
        } else if !line.starts_with(':') {
            let (field, value) = line.split_once(':').unwrap_or((line, ""));
            let value = value.strip_prefix(' ').unwrap_or(value);
            match field {
                "data" => {
                    self.data.push_str(value);
                    self.data.push('\n');
                    self.has_data = true;
                }
                "event" => self.event = value.to_owned(),
                // No reconnection/replay: event IDs and retry hints must not trigger a new inference.
                _ => {}
            }
        }
        self.line.clear();
        Ok(())
    }

    pub fn finish(&mut self) -> Result<(), Error> {
        if self.line.is_empty() && !self.has_data && self.event.is_empty() {
            Ok(())
        } else {
            Err(Error::Truncated)
        }
    }
}
