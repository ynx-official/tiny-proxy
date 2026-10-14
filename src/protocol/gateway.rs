//! Explicit text/image/function subset. Request conversion and stream projection are
//! pure so authentication, transport cancellation and UI cannot change semantics.
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, thiserror::Error)]
#[error("{message}: {param}")]
pub struct Error {
    pub message: &'static str,
    pub param: String,
    diagnostic_param: String,
    guidance: &'static str,
}
impl Error {
    fn at(mut self, parent: &str) -> Self {
        if parent.is_empty() {
            return self;
        }
        let container = (self.param.is_empty() && self.diagnostic_param.is_empty())
            || (self.param == "object" && self.diagnostic_param == "object");
        self.param = if container {
            parent.into()
        } else {
            format!("{parent}.{}", self.param)
        };
        self.diagnostic_param = if container {
            parent.into()
        } else {
            format!("{parent}.{}", self.diagnostic_param)
        };
        self
    }

    /// Only code-owned paths, array indices and rules may enter the journal.
    /// Arbitrary client keys are returned to that client, never persisted.
    pub fn diagnostic_message(&self) -> String {
        format!("参数 {}：{}", self.diagnostic_param, self.guidance)
    }
}
fn context<T>(parent: &str, operation: impl FnOnce() -> Result<T, Error>) -> Result<T, Error> {
    operation().map_err(|e| e.at(parent))
}
fn invalid(param: &'static str) -> Error {
    let guidance = match param {
        "object" => "此处必须是 JSON 对象才能转换；建议：检查请求结构",
        "model" => "model 必须是非空字符串；建议：提供模型 ID",
        "messages" => "messages 必须是数组才能转换；建议：提供 Chat 消息数组",
        "input" => "input 必须是字符串、输入对象或数组；建议：提供 Responses 输入结构",
        "role" => "Chat 消息缺少非空字符串 role；建议：提供消息角色",
        "content" => "内容无法转换成文本或受支持的图片；建议：提供字符串或对应内容块数组",
        "text" => "文本块的 text 必须是字符串才能转换；建议：提供文本字符串",
        "tools" => "tools 必须是数组才能转换；建议：提供 function 定义数组",
        "name" => "函数 name 必须是非空字符串才能转换；建议：提供函数名",
        "id" | "tool_call_id" => "调用 ID 必须是非空字符串才能转换；建议：提供调用 ID",
        "tool_calls" => "tool_calls 必须是数组才能转换；建议：提供调用数组",
        _ => "此处缺少协议转换所需的结构；建议：检查对应字段的基本格式",
    };
    invalid_because(param, guidance)
}
fn invalid_because(param: &'static str, guidance: &'static str) -> Error {
    Error {
        message: "Invalid or unsupported field",
        param: param.into(),
        diagnostic_param: param.into(),
        guidance,
    }
}
fn protocol() -> Error {
    Error {
        message: "Upstream stream failed integrity checks",
        param: String::new(),
        diagnostic_param: String::new(),
        guidance: "上游流完整性校验失败；建议：检查上游连接与终态事件",
    }
}

pub struct Request {
    pub body: Value,
    pub stream: bool,
    pub include_usage: bool,
    pub ignored_parameters: Vec<&'static str>,
}
// Downstream envelopes are trusted. Only inspect the structure needed for
// conversion; native values and opaque payloads are left for the upstream.
fn object(v: &Value) -> Result<&Map<String, Value>, Error> {
    v.as_object().ok_or_else(|| invalid("object"))
}
fn omit_nulls(v: &mut Value, optional: &[&str]) {
    if let Some(object) = v.as_object_mut() {
        object.retain(|key, value| !(value.is_null() && optional.contains(&key.as_str())));
    }
}
fn string<'a>(v: &'a Value, key: &'static str) -> Result<&'a str, Error> {
    v[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid(key))
}
fn boolean(v: &Value, key: &str, default: bool) -> bool {
    match &v[key] {
        Value::Bool(value) => *value,
        Value::String(value) => match value.trim().to_ascii_lowercase().as_str() {
            "true" | "1" => true,
            "false" | "0" => false,
            _ => default,
        },
        Value::Number(value) if value.as_u64() == Some(1) => true,
        Value::Number(value) if value.as_u64() == Some(0) => false,
        _ => default,
    }
}
fn ignored_parameters(v: &Value) -> Vec<&'static str> {
    // Only fixed names go into headers, never arbitrary client extension keys.
    let mut ignored = vec![];
    for key in [
        "max_tokens",
        "max_completion_tokens",
        "max_output_tokens",
        "temperature",
        "top_p",
        "stop",
        "seed",
        "logprobs",
        "top_logprobs",
        "frequency_penalty",
        "presence_penalty",
        "logit_bias",
        "response_format",
        "metadata",
        "user",
        "service_tier",
        "text",
        "truncation",
        "previous_response_id",
        "conversation",
        "background",
        "functions",
        "function_call",
    ] {
        if v.get(key).is_some_and(|value| !value.is_null()) {
            ignored.push(key);
        }
    }
    if v.get("n")
        .is_some_and(|v| !v.is_null() && v.as_u64() != Some(1))
    {
        ignored.push("n");
    }
    if v.get("store").is_some_and(|v| !v.is_null() && v != false) {
        ignored.push("store");
    }
    ignored
}
fn content(v: &Value, nullable: bool) -> Result<String, Error> {
    if v.is_null() && nullable {
        return Ok(String::new());
    }
    if let Some(s) = v.as_str() {
        return Ok(s.into());
    }
    let mut result = String::new();
    for (index, part) in v
        .as_array()
        .ok_or_else(|| invalid("content"))?
        .iter()
        .enumerate()
    {
        let text = context(&format!("content[{index}]"), || {
            object(part)?;
            if matches!(part["type"].as_str(), Some("image_url" | "input_image")) {
                return Err(invalid_because(
                    "type",
                    "此角色的内容无法转换成文本；建议：使用文本块，图片仅放入 user 消息",
                ));
            }
            part["text"].as_str().ok_or_else(|| invalid("text"))
        })?;
        result.push_str(text);
    }
    Ok(result)
}
fn input_image(part: &Value, url_field: &'static str) -> Result<Value, Error> {
    let image_error = || {
        invalid_because(
            url_field,
            "图片入口仅支持有效且非空的 PNG / JPEG / WebP / GIF Base64 data URL；建议：使用支持的 Base64 图片格式",
        )
    };
    let url = part[url_field].as_str().ok_or_else(image_error)?;
    let (prefix, encoded) = url.split_once(',').ok_or_else(image_error)?;
    let mime = prefix
        .strip_prefix("data:")
        .and_then(|p| p.strip_suffix(";base64"))
        .ok_or_else(image_error)?;
    // Keep the approved inline-image boundary: never fetch a URL or read a path.
    if !["image/png", "image/jpeg", "image/webp", "image/gif"].contains(&mime)
        || encoded.is_empty()
        || STANDARD.decode(encoded).is_err()
    {
        return Err(image_error());
    }
    let detail = part
        .get("detail")
        .filter(|v| !v.is_null())
        .cloned()
        .unwrap_or(json!("auto"));
    Ok(json!({"type":"input_image","image_url":url,"detail":detail}))
}
fn user_content(v: &Value) -> Result<Vec<Value>, Error> {
    if let Some(text) = v.as_str() {
        return Ok(if text.is_empty() {
            vec![]
        } else {
            vec![json!({"type":"input_text","text":text})]
        });
    }
    let mut result = vec![];
    for (index, part) in v
        .as_array()
        .ok_or_else(|| invalid("content"))?
        .iter()
        .enumerate()
    {
        let converted = context(&format!("content[{index}]"), || {
            object(part)?;
            match part["type"].as_str() {
                Some("image_url") if part["image_url"].is_string() => {
                    input_image(part, "image_url").map(Some)
                }
                Some("image_url") => context("image_url", || {
                    object(&part["image_url"])?;
                    input_image(&part["image_url"], "url").map(Some)
                }),
                Some("input_image") => input_image(part, "image_url").map(Some),
                _ => {
                    let text = part["text"].as_str().ok_or_else(|| invalid("text"))?;
                    Ok((!text.is_empty()).then(|| json!({"type":"input_text","text":text})))
                }
            }
        })?;
        if let Some(part) = converted {
            result.push(part);
        }
    }
    Ok(result)
}
fn args(v: &Value, key: &str) -> String {
    match v.get(key) {
        None => "{}".into(),
        Some(Value::String(value)) => value.clone(),
        Some(value) => value.to_string(),
    }
}
fn base(v: &Value) -> Result<Map<String, Value>, Error> {
    object(v)?;
    let model = string(v, "model")?;
    let mut body = json!({"model":model,"stream":true,"store":false,"instructions":"","include":["reasoning.encrypted_content"]}).as_object().unwrap().clone();
    if let Some(cache) = v.get("prompt_cache_key").filter(|v| !v.is_null()) {
        body.insert("prompt_cache_key".into(), cache.clone());
    }
    Ok(body)
}
fn tools(v: &Value, chat: bool, body: &mut Map<String, Value>) -> Result<(), Error> {
    if let Some(raw) = v.get("tools").filter(|v| !v.is_null()) {
        let raw = raw.as_array().ok_or_else(|| invalid("tools"))?;
        let mut converted = vec![];
        for (index, tool) in raw.iter().enumerate() {
            converted.push(context(&format!("tools[{index}]"), || {
                object(tool)?;
                let wrapped = chat && tool.get("function").is_some();
                let function = if wrapped { &tool["function"] } else { tool };
                context(if wrapped { "function" } else { "" }, || {
                    let definition = object(function)?;
                    string(function, "name")?;
                    let mut function = definition.clone();
                    function.retain(|key, value| {
                        !(["description", "strict"].contains(&key.as_str()) && value.is_null())
                    });
                    function
                        .entry("parameters")
                        .or_insert_with(|| json!({"type":"object","properties":{}}));
                    function.insert("type".into(), json!("function"));
                    Ok(Value::Object(function))
                })
            })?);
        }
        body.insert("tools".into(), json!(converted));
    }
    if let Some(choice) = v.get("tool_choice").filter(|v| !v.is_null()) {
        let converted = if chat && choice["function"]["name"].is_string() {
            json!({"type":"function","name":choice["function"]["name"]})
        } else {
            choice.clone()
        };
        body.insert("tool_choice".into(), converted);
    }
    if v.get("parallel_tool_calls").is_some_and(|v| !v.is_null()) {
        body.insert(
            "parallel_tool_calls".into(),
            json!(boolean(v, "parallel_tool_calls", false)),
        );
    }
    Ok(())
}
pub fn normalize_chat(v: Value) -> Result<Request, Error> {
    let mut body = base(&v)?;
    let mut ignored_parameters = ignored_parameters(&v);
    let mut input = vec![];
    let mut instructions = vec![];
    let messages = v["messages"]
        .as_array()
        .ok_or_else(|| invalid("messages"))?;
    for (index, message) in messages.iter().enumerate() {
        context(&format!("messages[{index}]"), || {
            object(message)?;
            let role = string(message, "role")?;
            if role == "assistant"
                && message
                    .get("reasoning_content")
                    .is_some_and(|v| !v.is_null() && v != "")
                && !ignored_parameters.contains(&"messages[].reasoning_content")
            {
                ignored_parameters.push("messages[].reasoning_content");
            }
            match role {
                "system" | "developer" => {
                    let text = content(&message["content"], false)?;
                    if input.is_empty() {
                        instructions.push(text);
                    } else {
                        input.push(json!({"type":"message","role":role,"content":[{"type":"input_text","text":text}]}));
                    }
                }
                "tool" => {
                    let id = string(message, "tool_call_id")?;
                    input.push(json!({"type":"function_call_output","call_id":id,"output":content(&message["content"],false)?}));
                }
                _ => {
                    let mut parts = if role == "user" {
                        user_content(&message["content"])?
                    } else {
                        let text = content(&message["content"], true)?;
                        if text.is_empty() {
                            vec![]
                        } else {
                            vec![
                                json!({"type":if role == "assistant" {"output_text"} else {"input_text"},"text":text}),
                            ]
                        }
                    };
                    if role == "assistant"
                        && let Some(refusal) = message["refusal"].as_str().filter(|s| !s.is_empty())
                    {
                        parts.push(json!({"type":"refusal","refusal":refusal}));
                    }
                    if !parts.is_empty() {
                        input.push(json!({"type":"message","role":role,"content":parts}));
                    }
                    if role == "assistant"
                        && let Some(raw) = message.get("tool_calls").filter(|v| !v.is_null())
                    {
                        for (index, call) in raw
                            .as_array()
                            .ok_or_else(|| invalid("tool_calls"))?
                            .iter()
                            .enumerate()
                        {
                            context(&format!("tool_calls[{index}]"), || {
                                object(call)?;
                                let id = string(call, "id")?;
                                let function = &call["function"];
                                let (name, arguments) = context("function", || {
                                    object(function)?;
                                    Ok((string(function, "name")?, args(function, "arguments")))
                                })?;
                                input.push(json!({"type":"function_call","call_id":id,"name":name,"arguments":arguments}));
                                Ok(())
                            })?;
                        }
                    }
                }
            }
            Ok(())
        })?;
    }
    body.insert("instructions".into(), json!(instructions.join("\n\n")));
    body.insert("input".into(), json!(input));
    tools(&v, true, &mut body)?;
    if let Some(effort) = v.get("reasoning_effort").filter(|v| !v.is_null()) {
        body.insert(
            "reasoning".into(),
            json!({"effort":effort,"summary":"auto"}),
        );
    }
    let stream = boolean(&v, "stream", false);
    let include_usage = stream && boolean(&v["stream_options"], "include_usage", false);
    Ok(Request {
        body: Value::Object(body),
        stream,
        include_usage,
        ignored_parameters,
    })
}
pub fn normalize_responses(mut v: Value) -> Result<Request, Error> {
    let mut body = base(&v)?;
    let ignored_parameters = ignored_parameters(&v);
    let mut input = match &v["input"] {
        Value::String(text) => json!([{"role":"user","content":text}]),
        Value::Object(_) => json!([v["input"].clone()]),
        Value::Array(_) => v["input"].clone(),
        _ => return Err(invalid("input")),
    };
    // Native items are already in the upstream format. Do not revalidate enums,
    // text values, tool links, schemas, status or opaque reasoning replay data.
    for (index, item) in input.as_array_mut().unwrap().iter_mut().enumerate() {
        if item["role"] == "user" {
            if let Some(parts) = item["content"].as_array_mut() {
                for (part_index, part) in parts.iter_mut().enumerate() {
                    if part["type"] == "input_image" {
                        let converted =
                            context(&format!("input[{index}].content[{part_index}]"), || {
                                input_image(part, "image_url")
                            })?;
                        *part = converted;
                    }
                }
            }
        } else if item["content"]
            .as_array()
            .is_some_and(|parts| parts.iter().any(|p| p["type"] == "input_image"))
        {
            return Err(invalid_because(
                "content",
                "图片入口仅支持 user 消息；建议：将图片放入 user 消息",
            )
            .at(&format!("input[{index}]")));
        }
    }
    body.insert("input".into(), input);
    for key in ["instructions", "include"] {
        if let Some(value) = v.get(key).filter(|v| !v.is_null()) {
            body.insert(key.into(), value.clone());
        }
    }
    if let Some(reasoning) = v.get_mut("reasoning").filter(|v| !v.is_null()) {
        omit_nulls(reasoning, &["effort", "summary"]);
        body.insert("reasoning".into(), reasoning.clone());
    }
    tools(&v, false, &mut body)?;
    Ok(Request {
        body: Value::Object(body),
        stream: boolean(&v, "stream", false),
        include_usage: false,
        ignored_parameters,
    })
}

#[derive(Default)]
struct Tool {
    index: usize,
    id: String,
    name: String,
    args: String,
    emitted: usize,
    announced: bool,
}
struct OutputItem {
    id: Option<String>,
    kind: String,
    done: Option<Value>,
}
pub struct Projection {
    id: String,
    created: i64,
    model: String,
    started: bool,
    text: String,
    refusal: String,
    parts: BTreeMap<(u64, u64), String>,
    tools: BTreeMap<u64, Tool>,
    items: BTreeMap<u64, OutputItem>,
    terminal: Option<Value>,
    reason: String,
    include_usage: bool,
    bytes: usize,
}
impl Projection {
    pub fn new(model: String, include_usage: bool) -> Self {
        Self {
            id: String::new(),
            created: 0,
            model,
            started: false,
            text: String::new(),
            refusal: String::new(),
            parts: BTreeMap::new(),
            tools: BTreeMap::new(),
            items: BTreeMap::new(),
            terminal: None,
            reason: String::new(),
            include_usage,
            bytes: 0,
        }
    }
    fn chunk(&self, delta: Value, finish: Value) -> Value {
        json!({"id":self.id,"object":"chat.completion.chunk","created":self.created,"model":self.model,"choices":[{"index":0,"delta":delta,"finish_reason":finish}]})
    }
    fn identity(&mut self, response: &Value) -> Result<(), Error> {
        let id = string(response, "id")?;
        if !self.id.is_empty() && self.id != id {
            return Err(protocol());
        }
        self.id = id.into();
        if let Some(time) = response["created_at"].as_i64() {
            if self.created != 0 && self.created != time {
                return Err(protocol());
            }
            self.created = time;
        }
        if let Some(model) = response["model"].as_str() {
            self.model = model.into();
        }
        Ok(())
    }
    fn update_tool(
        &mut self,
        index: u64,
        item: Option<&Value>,
        delta: Option<&str>,
        snapshot: Option<&str>,
        out: &mut Vec<Value>,
    ) -> Result<(), Error> {
        let next_index = self.tools.len();
        let t = self.tools.entry(index).or_insert_with(|| Tool {
            index: next_index,
            ..Tool::default()
        });
        if let Some(item) = item {
            for (field, target) in [("call_id", &mut t.id), ("name", &mut t.name)] {
                if let Some(s) = item[field].as_str() {
                    if !target.is_empty() && target != s {
                        return Err(protocol());
                    }
                    *target = s.into();
                }
            }
        }
        if let Some(delta) = delta {
            t.args.push_str(delta);
        }
        if let Some(snapshot) = snapshot {
            if !snapshot.starts_with(&t.args) {
                return Err(protocol());
            }
            t.args = snapshot.into();
        }
        let mut changes = vec![];
        if !t.id.is_empty() && !t.name.is_empty() {
            if !t.announced {
                changes.push(json!({"index":t.index,"id":t.id,"type":"function","function":{"name":t.name,"arguments":""}}));
                t.announced = true;
            }
            if t.args.len() > t.emitted {
                changes
                    .push(json!({"index":t.index,"function":{"arguments":&t.args[t.emitted..]}}));
                t.emitted = t.args.len();
            }
        }
        for change in changes {
            out.push(self.chunk(json!({"tool_calls":[change]}), Value::Null));
        }
        Ok(())
    }
    pub fn accept(&mut self, v: &Value) -> Result<Vec<Value>, Error> {
        if self.terminal.is_some() {
            return Err(protocol());
        }
        self.bytes += v.to_string().len();
        if self.bytes > 16 * 1024 * 1024 {
            return Err(protocol());
        }
        let kind = string(v, "type")?;
        if kind == "error" || kind == "response.failed" {
            return Err(protocol());
        }
        if v.get("response").is_some() {
            self.identity(&v["response"])?;
        }
        let mut out = vec![];
        if !self.started && !self.id.is_empty() {
            self.started = true;
            out.push(self.chunk(json!({"role":"assistant","content":""}), Value::Null));
        }
        if !self.started {
            return Err(protocol());
        }
        match kind {
            "response.created" | "response.in_progress" | "response.queued" => {}
            "response.output_text.delta" => {
                let delta = v["delta"].as_str().ok_or_else(protocol)?;
                let key = (index(v)?, v["content_index"].as_u64().ok_or_else(protocol)?);
                self.parts.entry(key).or_default().push_str(delta);
                self.text.push_str(delta);
                out.push(self.chunk(json!({"content":delta}), Value::Null));
            }
            "response.refusal.delta" => {
                let s = v["delta"].as_str().ok_or_else(protocol)?;
                self.refusal.push_str(s);
                out.push(self.chunk(json!({"refusal":s}), Value::Null));
            }
            "response.function_call_arguments.delta" => {
                self.update_tool(
                    index(v)?,
                    None,
                    Some(v["delta"].as_str().ok_or_else(protocol)?),
                    None,
                    &mut out,
                )?;
            }
            "response.function_call_arguments.done" => {
                self.update_tool(
                    index(v)?,
                    None,
                    None,
                    Some(v["arguments"].as_str().ok_or_else(protocol)?),
                    &mut out,
                )?;
            }
            "response.output_item.added" | "response.output_item.done" => {
                let item = &v["item"];
                let item_kind = string(item, "type")?;
                let item_id = item["id"].as_str();
                let slot = self.items.entry(index(v)?).or_insert_with(|| OutputItem {
                    id: item_id.map(str::to_owned),
                    kind: item_kind.into(),
                    done: None,
                });
                if slot.kind != item_kind
                    || slot.done.is_some()
                    || matches!((&slot.id, item_id), (Some(previous), Some(next)) if previous != next)
                {
                    return Err(protocol());
                }
                if slot.id.is_none() {
                    slot.id = item_id.map(str::to_owned);
                }
                if kind.ends_with(".done") {
                    slot.done = Some(item.clone());
                }
                match item["type"].as_str() {
                    Some("function_call") => self.update_tool(
                        index(v)?,
                        Some(item),
                        None,
                        if kind.ends_with(".done") {
                            item["arguments"].as_str()
                        } else {
                            None
                        },
                        &mut out,
                    )?,
                    Some("message" | "reasoning") => {}
                    _ => return Err(protocol()),
                }
            }
            "response.completed" | "response.incomplete" => {
                let mut response = v["response"].clone();
                let complete = kind == "response.completed";
                if response["status"] != if complete { "completed" } else { "incomplete" } {
                    return Err(protocol());
                }
                // Codex can omit repeated output in its completed envelope.
                // Reconstruct only from finished, contiguous items in this same
                // response; deltas alone and unfinished items are insufficient.
                if complete
                    && response["output"].as_array().is_some_and(Vec::is_empty)
                    && !self.items.is_empty()
                {
                    let mut output = Vec::new();
                    for (position, (index, slot)) in self.items.iter().enumerate() {
                        if *index != position as u64 {
                            return Err(protocol());
                        }
                        let item = slot.done.as_ref().ok_or_else(protocol)?;
                        if slot.kind == "message" && item["status"] != "completed" {
                            return Err(protocol());
                        }
                        output.push(item.clone());
                    }
                    if self
                        .parts
                        .keys()
                        .any(|(index, _)| !self.items.contains_key(index))
                    {
                        return Err(protocol());
                    }
                    response["output"] = json!(output);
                }
                let mut final_text = String::new();
                let mut final_refusal = String::new();
                let mut final_calls = HashSet::new();
                for (position, item) in response["output"]
                    .as_array()
                    .ok_or_else(protocol)?
                    .iter()
                    .enumerate()
                {
                    match item["type"].as_str() {
                        Some("message") => {
                            for part in item["content"].as_array().ok_or_else(protocol)? {
                                match part["type"].as_str() {
                                    Some("output_text") => final_text
                                        .push_str(part["text"].as_str().ok_or_else(protocol)?),
                                    Some("refusal") => final_refusal
                                        .push_str(part["refusal"].as_str().ok_or_else(protocol)?),
                                    _ => return Err(protocol()),
                                }
                            }
                        }
                        Some("function_call") => {
                            let id = string(item, "call_id")?;
                            if !final_calls.insert(id.to_owned()) {
                                return Err(protocol());
                            }
                            let key = self
                                .tools
                                .iter()
                                .find(|(_, t)| t.id == id)
                                .map(|(k, _)| *k)
                                .unwrap_or(position as u64);
                            if complete {
                                // Trusting client history does not make a malformed
                                // upstream tool result a valid completed response.
                                let arguments = item["arguments"].as_str().ok_or_else(protocol)?;
                                if !serde_json::from_str::<Value>(arguments)
                                    .is_ok_and(|v| v.is_object())
                                {
                                    return Err(protocol());
                                }
                            }
                            self.update_tool(
                                key,
                                Some(item),
                                None,
                                Some(item["arguments"].as_str().ok_or_else(protocol)?),
                                &mut out,
                            )?;
                        }
                        Some("reasoning") => {}
                        _ => return Err(protocol()),
                    }
                }
                if self
                    .tools
                    .values()
                    .any(|t| !t.announced || !final_calls.contains(&t.id))
                {
                    return Err(protocol());
                }
                if !final_text.starts_with(&self.text) || !final_refusal.starts_with(&self.refusal)
                {
                    return Err(protocol());
                }
                if final_text.len() > self.text.len() {
                    out.push(self.chunk(
                        json!({"content":&final_text[self.text.len()..]}),
                        Value::Null,
                    ));
                }
                if final_refusal.len() > self.refusal.len() {
                    out.push(self.chunk(
                        json!({"refusal":&final_refusal[self.refusal.len()..]}),
                        Value::Null,
                    ));
                }
                self.text = final_text;
                self.refusal = final_refusal;
                self.reason = if complete {
                    if self.tools.is_empty() {
                        "stop"
                    } else {
                        "tool_calls"
                    }
                } else {
                    match response["incomplete_details"]["reason"].as_str() {
                        Some("max_output_tokens") => "length",
                        Some("content_filter") => "content_filter",
                        _ => return Err(protocol()),
                    }
                }
                .into();
                out.push(self.chunk(json!({}), json!(self.reason)));
                if self.include_usage
                    && let Some(usage) = usage(&response)
                {
                    out.push(json!({"id":self.id,"object":"chat.completion.chunk","created":self.created,"model":self.model,"choices":[],"usage":usage}));
                }
                self.terminal = Some(response);
            }
            // These events carry part boundaries/annotations/reasoning; native
            // Responses forwards them unchanged. Chat exposes text/functions.
            "response.output_text.done"
            | "response.refusal.done"
            | "response.content_part.added"
            | "response.content_part.done"
            | "response.reasoning_summary_part.added"
            | "response.reasoning_summary_part.done"
            | "response.reasoning_summary_text.delta"
            | "response.reasoning_summary_text.done"
            | "response.reasoning_text.delta"
            | "response.reasoning_text.done"
            | "response.output_text.annotation.added" => {}
            _ => return Err(protocol()),
        }
        Ok(out)
    }
    pub fn terminal(&self) -> Option<&Value> {
        self.terminal.as_ref()
    }
    pub fn finish(&self) -> Result<Value, Error> {
        let response = self.terminal.as_ref().ok_or_else(protocol)?;
        let mut message = json!({"role":"assistant","content":if self.text.is_empty() && !self.tools.is_empty() {Value::Null} else {json!(self.text)}});
        if !self.refusal.is_empty() {
            message["refusal"] = json!(self.refusal);
        }
        if !self.tools.is_empty() {
            let mut tools: Vec<_> = self.tools.values().collect();
            tools.sort_by_key(|t| t.index);
            message["tool_calls"] = json!(tools.iter().map(|t| json!({"id":t.id,"type":"function","function":{"name":t.name,"arguments":t.args}})).collect::<Vec<_>>());
        }
        let mut result = json!({"id":self.id,"object":"chat.completion","created":self.created,"model":self.model,"choices":[{"index":0,"message":message,"finish_reason":self.reason}]});
        if let Some(usage) = usage(response) {
            result["usage"] = usage;
        }
        Ok(result)
    }
}
fn index(v: &Value) -> Result<u64, Error> {
    v["output_index"].as_u64().ok_or_else(protocol)
}
fn usage(response: &Value) -> Option<Value> {
    let u = response.get("usage")?.as_object()?;
    let mut out = Map::new();
    for (from, to) in [
        ("input_tokens", "prompt_tokens"),
        ("output_tokens", "completion_tokens"),
        ("total_tokens", "total_tokens"),
        ("input_tokens_details", "prompt_tokens_details"),
        ("output_tokens_details", "completion_tokens_details"),
    ] {
        if let Some(v) = u.get(from) {
            out.insert(to.into(), v.clone());
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(Value::Object(out))
    }
}
