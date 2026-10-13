use super::sse::Event;
use serde_json::Value;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("upstream stream ended without a terminal response")]
    Interrupted,
    #[error("invalid upstream response event")]
    Protocol,
    #[error("upstream response failed")]
    Failed,
    #[error("upstream response was incomplete")]
    Incomplete,
    #[error("upstream output exceeded the configured budget")]
    TooLarge,
}

/// P0 text-only probe. This is not the later Chat/tool-call conversion state machine.
pub struct TextProbe {
    limit: usize,
    text: String,
    response_id: Option<String>,
    completed: Option<Value>,
    failed: bool,
}
impl TextProbe {
    pub fn new(limit: usize) -> Self {
        Self {
            limit,
            text: String::new(),
            response_id: None,
            completed: None,
            failed: false,
        }
    }
    pub fn accept(&mut self, event: Event) -> Result<(), Error> {
        let result = self.process(event);
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    fn process(&mut self, event: Event) -> Result<(), Error> {
        if self.failed || self.completed.is_some() {
            return Err(Error::Protocol);
        }
        let value: Value = serde_json::from_str(&event.data).map_err(|_| Error::Protocol)?;
        let kind = value["type"].as_str().ok_or(Error::Protocol)?;
        if event.event != "message" && event.event != kind {
            return Err(Error::Protocol);
        }
        match kind {
            "response.created" | "response.in_progress" => {
                let id = value["response"]["id"]
                    .as_str()
                    .filter(|v| !v.is_empty())
                    .ok_or(Error::Protocol)?;
                if self.response_id.as_deref().is_some_and(|old| old != id) {
                    return Err(Error::Protocol);
                }
                self.response_id = Some(id.to_owned());
            }
            "response.output_text.delta" => {
                let delta = value["delta"].as_str().ok_or(Error::Protocol)?;
                if self.text.len() + delta.len() > self.limit {
                    return Err(Error::TooLarge);
                }
                self.text.push_str(delta);
            }
            "response.completed" => {
                let response = &value["response"];
                let id = response["id"]
                    .as_str()
                    .filter(|v| !v.is_empty())
                    .ok_or(Error::Protocol)?;
                if response["status"] != "completed"
                    || self.response_id.as_deref().is_some_and(|old| old != id)
                {
                    return Err(Error::Protocol);
                }
                let items = response["output"].as_array().ok_or(Error::Protocol)?;
                let mut final_text = String::new();
                for item in items {
                    match item["type"].as_str() {
                        Some("reasoning") => continue,
                        Some("message") => {
                            for content in item["content"].as_array().ok_or(Error::Protocol)? {
                                if content["type"] != "output_text" {
                                    return Err(Error::Protocol);
                                }
                                let text = content["text"].as_str().ok_or(Error::Protocol)?;
                                if final_text.len() + text.len() > self.limit {
                                    return Err(Error::TooLarge);
                                }
                                final_text.push_str(text);
                            }
                        }
                        _ => return Err(Error::Protocol),
                    }
                }
                if !final_text.starts_with(&self.text) {
                    return Err(Error::Protocol);
                }
                self.text = final_text;
                self.completed = Some(response.clone());
            }
            "response.failed" | "error" => return Err(Error::Failed),
            "response.incomplete" => return Err(Error::Incomplete),
            "response.output_item.added" | "response.output_item.done" => {
                if !matches!(
                    value["item"]["type"].as_str(),
                    Some("message" | "reasoning")
                ) {
                    return Err(Error::Protocol);
                }
            }
            "response.content_part.added"
            | "response.content_part.done"
            | "response.output_text.done"
            | "response.reasoning_summary_part.added"
            | "response.reasoning_summary_part.done"
            | "response.reasoning_summary_text.delta"
            | "response.reasoning_summary_text.done" => {}
            _ => return Err(Error::Protocol),
        }
        Ok(())
    }
    pub fn finish(self) -> Result<(String, Value), Error> {
        if self.failed {
            return Err(Error::Failed);
        }
        Ok((self.text, self.completed.ok_or(Error::Interrupted)?))
    }
}
