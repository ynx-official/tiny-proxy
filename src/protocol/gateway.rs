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
}
fn invalid(param: &str) -> Error {
    Error {
        message: "Invalid or unsupported field",
        param: param.into(),
    }
}
fn protocol() -> Error {
    Error {
        message: "Upstream stream failed integrity checks",
        param: String::new(),
    }
}

pub struct Request {
    pub body: Value,
    pub stream: bool,
    pub include_usage: bool,
    pub ignored_parameters: Vec<&'static str>,
}
fn fields(v: &Value, allowed: &[&str]) -> Result<(), Error> {
    let obj = v.as_object().ok_or_else(|| invalid("object"))?;
    for key in obj.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(invalid(key));
        }
    }
    Ok(())
}
fn omit_nulls(v: &mut Value, optional: &[&str]) {
    // Only normalize named envelope fields. Recursive cleanup would corrupt
    // tool arguments, JSON Schema constants/defaults and opaque replay data.
    if let Some(object) = v.as_object_mut() {
        object.retain(|key, value| !(value.is_null() && optional.contains(&key.as_str())));
    }
}
fn string<'a>(v: &'a Value, key: &str) -> Result<&'a str, Error> {
    v[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| invalid(key))
}
fn boolean(v: &Value, key: &str, default: bool) -> Result<bool, Error> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v.as_bool().ok_or_else(|| invalid(key)),
    }
}
fn token_limits(v: &Value, keys: &[&'static str]) -> Result<Vec<&'static str>, Error> {
    let mut ignored = Vec::new();
    for &key in keys {
        if let Some(value) = v.get(key).filter(|value| !value.is_null()) {
            if value.as_u64().is_none_or(|limit| limit == 0) {
                return Err(invalid(key));
            }
            // Codex rejects output caps. Keep their names for the HTTP warning,
            // rather than forwarding them or truncating text/tool arguments.
            ignored.push(key);
        }
    }
    Ok(ignored)
}
fn name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
fn content(v: &Value, nullable: bool) -> Result<String, Error> {
    if v.is_null() && nullable {
        return Ok(String::new());
    }
    if let Some(s) = v.as_str() {
        return Ok(s.into());
    }
    let mut result = String::new();
    for part in v.as_array().ok_or_else(|| invalid("content"))? {
        fields(part, &["type", "text"])?;
        if part["type"] != "text" {
            return Err(invalid("content.type"));
        }
        result.push_str(
            part["text"]
                .as_str()
                .ok_or_else(|| invalid("content.text"))?,
        );
    }
    Ok(result)
}

fn input_image(part: &Value) -> Result<Value, Error> {
    fields(part, &["type", "image_url", "detail"])?;
    let url = string(part, "image_url")?;
    let (prefix, encoded) = url.split_once(',').ok_or_else(|| invalid("image_url"))?;
    let mime = prefix
        .strip_prefix("data:")
        .and_then(|prefix| prefix.strip_suffix(";base64"))
        .ok_or_else(|| invalid("image_url"))?;
    // Only inline bytes cross this boundary. Never fetch a client URL or read a
    // local path. The HTTP layer bounds the entire JSON (including Base64) to 2 MiB.
    if !["image/png", "image/jpeg", "image/webp", "image/gif"].contains(&mime)
        || encoded.is_empty()
        || STANDARD.decode(encoded).is_err()
    {
        return Err(invalid("image_url"));
    }
    let detail = match part.get("detail") {
        None => "auto",
        Some(value) => value
            .as_str()
            .filter(|value| ["auto", "low", "high", "original"].contains(value))
            .ok_or_else(|| invalid("image_url.detail"))?,
    };
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
    for part in v.as_array().ok_or_else(|| invalid("content"))? {
        match part["type"].as_str() {
            Some("text") => {
                fields(part, &["type", "text"])?;
                let text = part["text"]
                    .as_str()
                    .ok_or_else(|| invalid("content.text"))?;
                if !text.is_empty() {
                    result.push(json!({"type":"input_text","text":text}));
                }
            }
            Some("image_url") => {
                fields(part, &["type", "image_url"])?;
                let image = &part["image_url"];
                fields(image, &["url", "detail"])?;
                let mut converted = json!({"type":"input_image","image_url":image["url"]});
                if let Some(detail) = image.get("detail") {
                    converted["detail"] = detail.clone();
                }
                result.push(input_image(&converted)?);
            }
            _ => return Err(invalid("content.type")),
        }
    }
    Ok(result)
}

fn args(v: &Value, key: &str) -> Result<String, Error> {
    let s = v[key].as_str().ok_or_else(|| invalid(key))?;
    if !serde_json::from_str::<Value>(s).is_ok_and(|v| v.is_object()) {
        return Err(invalid(key));
    }
    Ok(s.into())
}
fn base(v: &Value) -> Result<Map<String, Value>, Error> {
    let model = string(v, "model")?;
    if model.len() > 200 || model.chars().any(char::is_control) {
        return Err(invalid("model"));
    }
    if boolean(v, "store", false)? {
        return Err(invalid("store"));
    }
    let mut b = json!({"model":model,"stream":true,"store":false,"instructions":"","include":["reasoning.encrypted_content"]}).as_object().unwrap().clone();
    if let Some(key) = v.get("prompt_cache_key") {
        let s = key
            .as_str()
            .filter(|s| {
                !s.is_empty()
                    && s.len() <= 256
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_:./".contains(&b))
            })
            .ok_or_else(|| invalid("prompt_cache_key"))?;
        b.insert("prompt_cache_key".into(), json!(s));
    }
    Ok(b)
}
fn tools(v: &Value, chat: bool, b: &mut Map<String, Value>) -> Result<(), Error> {
    let mut names = HashSet::new();
    if let Some(raw) = v.get("tools") {
        let raw = raw
            .as_array()
            .filter(|a| a.len() <= 128)
            .ok_or_else(|| invalid("tools"))?;
        let mut converted = vec![];
        for tool in raw {
            let function = if chat {
                fields(tool, &["type", "function"])?;
                &tool["function"]
            } else {
                tool
            };
            if tool["type"] != "function" {
                return Err(invalid("tools.type"));
            }
            fields(
                function,
                if chat {
                    &["name", "description", "parameters", "strict"]
                } else {
                    &["type", "name", "description", "parameters", "strict"]
                },
            )?;
            let n = string(function, "name")?;
            if !name(n) || !names.insert(n.to_owned()) {
                return Err(invalid("tools.name"));
            }
            if function.get("parameters").is_some_and(|v| !v.is_object()) {
                return Err(invalid("tools.parameters"));
            }
            if function
                .get("description")
                .is_some_and(|v| !v.is_null() && !v.is_string())
            {
                return Err(invalid("tools.description"));
            }
            if function
                .get("strict")
                .is_some_and(|v| !v.is_null() && !v.is_boolean())
            {
                return Err(invalid("tools.strict"));
            }
            let mut f = function.as_object().unwrap().clone();
            f.retain(|key, value| {
                !(["description", "strict"].contains(&key.as_str()) && value.is_null())
            });
            f.entry("parameters")
                .or_insert_with(|| json!({"type":"object","properties":{}}));
            f.insert("type".into(), json!("function"));
            converted.push(Value::Object(f));
        }
        b.insert("tools".into(), json!(converted));
    }
    if let Some(choice) = v.get("tool_choice") {
        let converted = if let Some(s) = choice.as_str() {
            if !["auto", "none", "required"].contains(&s) || (s == "required" && names.is_empty()) {
                return Err(invalid("tool_choice"));
            }
            choice.clone()
        } else {
            fields(
                choice,
                if chat {
                    &["type", "function"]
                } else {
                    &["type", "name"]
                },
            )?;
            if choice["type"] != "function" {
                return Err(invalid("tool_choice"));
            }
            let f = if chat {
                fields(&choice["function"], &["name"])?;
                &choice["function"]
            } else {
                choice
            };
            let n = string(f, "name")?;
            if !names.contains(n) {
                return Err(invalid("tool_choice.name"));
            }
            json!({"type":"function","name":n})
        };
        b.insert("tool_choice".into(), converted);
    }
    if v.get("parallel_tool_calls").is_some() {
        b.insert(
            "parallel_tool_calls".into(),
            json!(boolean(v, "parallel_tool_calls", false)?),
        );
    }
    Ok(())
}

pub fn normalize_chat(mut v: Value) -> Result<Request, Error> {
    omit_nulls(
        &mut v,
        &[
            "stream",
            "store",
            "tools",
            "tool_choice",
            "parallel_tool_calls",
            "prompt_cache_key",
            "stream_options",
            "reasoning_effort",
            "n",
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
        ],
    );
    fields(
        &v,
        &[
            "model",
            "messages",
            "stream",
            "stream_options",
            "tools",
            "tool_choice",
            "parallel_tool_calls",
            "reasoning_effort",
            "max_tokens",
            "max_completion_tokens",
            "n",
            "store",
            "prompt_cache_key",
        ],
    )?;
    let mut ignored_parameters = token_limits(&v, &["max_tokens", "max_completion_tokens"])?;
    if v.get("n").is_some_and(|n| n.as_u64() != Some(1)) {
        return Err(invalid("n"));
    }
    let mut b = base(&v)?;
    let mut input = vec![];
    let mut instructions = vec![];
    let mut instruction_role = None;
    let mut calls = BTreeMap::new();
    let mut pending = HashSet::new();
    let messages = v["messages"]
        .as_array()
        .filter(|m| !m.is_empty())
        .ok_or_else(|| invalid("messages"))?;
    for m in messages {
        fields(
            m,
            &[
                "role",
                "content",
                "tool_calls",
                "tool_call_id",
                "reasoning_content",
                "name",
                "refusal",
                "audio",
                "function_call",
                "annotations",
            ],
        )?;
        let role = string(m, "role")?;
        for field in ["refusal", "audio", "function_call", "annotations"] {
            if let Some(value) = m.get(field) {
                let valid = value.is_null()
                    || match field {
                        "refusal" => value.is_string(),
                        "annotations" => value.as_array().is_some_and(Vec::is_empty),
                        _ => false,
                    };
                if role != "assistant" || !valid {
                    return Err(invalid(field));
                }
            }
        }
        if role != "tool" && m.get("name").is_some() {
            return Err(invalid("messages.name"));
        }
        if let Some(reasoning) = m.get("reasoning_content") {
            if role != "assistant" || !(reasoning.is_null() || reasoning.is_string()) {
                return Err(invalid("reasoning_content"));
            }
            // Chat reasoning text cannot stand in for a Codex encrypted replay
            // item. Accept client history metadata without injecting it into
            // instructions/content, and report nonempty omissions once per request.
            if reasoning.as_str().is_some_and(|text| !text.is_empty())
                && !ignored_parameters.contains(&"messages[].reasoning_content")
            {
                ignored_parameters.push("messages[].reasoning_content");
            }
        }
        if role != "assistant" && m.get("tool_calls").is_some() {
            return Err(invalid("tool_calls"));
        }
        if role != "tool" && m.get("tool_call_id").is_some() {
            return Err(invalid("tool_call_id"));
        }
        match role {
            "system" | "developer" => {
                if !input.is_empty() || instruction_role.is_some_and(|r| r != role) {
                    return Err(invalid("messages.role"));
                }
                instruction_role = Some(role);
                instructions.push(content(&m["content"], false)?);
            }
            "user" | "assistant" => {
                if !pending.is_empty() {
                    return Err(invalid("messages.tool_call_id"));
                }
                let mut c = if role == "user" {
                    user_content(&m["content"])?
                } else {
                    let text = content(&m["content"], true)?;
                    if text.is_empty() {
                        vec![]
                    } else {
                        vec![json!({"type":"output_text","text":text})]
                    }
                };
                if let Some(refusal) = m["refusal"].as_str().filter(|s| !s.is_empty()) {
                    c.push(json!({"type":"refusal","refusal":refusal}));
                }
                if !c.is_empty() {
                    input.push(json!({"type":"message","role":role,"content":c}));
                }
                if let Some(tc) = m.get("tool_calls").filter(|v| !v.is_null()) {
                    for call in tc.as_array().ok_or_else(|| invalid("tool_calls"))? {
                        fields(call, &["id", "type", "function", "index"])?;
                        // Clients often retain the stream assembly index in
                        // history. Array order and call IDs remain authoritative.
                        if call
                            .get("index")
                            .is_some_and(|v| !v.is_null() && v.as_u64().is_none())
                        {
                            return Err(invalid("messages[].tool_calls[].index"));
                        }
                        fields(&call["function"], &["name", "arguments"])?;
                        let id = string(call, "id")?;
                        let n = string(&call["function"], "name")?;
                        if call["type"] != "function"
                            || !name(n)
                            || calls.insert(id.to_owned(), n.to_owned()).is_some()
                        {
                            return Err(invalid("tool_calls"));
                        }
                        pending.insert(id.to_owned());
                        input.push(json!({"type":"function_call","call_id":id,"name":n,"arguments":args(&call["function"],"arguments")?}));
                    }
                }
            }
            "tool" => {
                let id = string(m, "tool_call_id")?;
                if !pending.remove(id) {
                    return Err(invalid("tool_call_id"));
                }
                if let Some(tool_name) = m.get("name").filter(|v| !v.is_null())
                    && tool_name.as_str() != calls.get(id).map(String::as_str)
                {
                    return Err(invalid("messages.name"));
                }
                input.push(json!({"type":"function_call_output","call_id":id,"output":content(&m["content"],false)?}));
            }
            _ => return Err(invalid("messages.role")),
        }
    }
    if !pending.is_empty() || input.is_empty() {
        return Err(invalid("messages"));
    }
    b.insert("instructions".into(), json!(instructions.join("\n\n")));
    b.insert("input".into(), json!(input));
    tools(&v, true, &mut b)?;
    if let Some(effort) = v.get("reasoning_effort") {
        if !effort.is_string() {
            return Err(invalid("reasoning_effort"));
        }
        b.insert(
            "reasoning".into(),
            json!({"effort":effort,"summary":"auto"}),
        );
    }
    let stream = boolean(&v, "stream", false)?;
    let include_usage = if let Some(o) = v.get("stream_options") {
        fields(o, &["include_usage"])?;
        if !stream {
            return Err(invalid("stream_options"));
        }
        boolean(o, "include_usage", false)?
    } else {
        false
    };
    Ok(Request {
        body: Value::Object(b),
        stream,
        include_usage,
        ignored_parameters,
    })
}

pub fn normalize_responses(mut v: Value) -> Result<Request, Error> {
    omit_nulls(
        &mut v,
        &[
            "stream",
            "store",
            "tools",
            "tool_choice",
            "parallel_tool_calls",
            "prompt_cache_key",
            "instructions",
            "reasoning",
            "include",
            "temperature",
            "top_p",
            "metadata",
            "user",
            "service_tier",
            "text",
            "truncation",
        ],
    );
    fields(
        &v,
        &[
            "model",
            "input",
            "instructions",
            "stream",
            "store",
            "tools",
            "tool_choice",
            "parallel_tool_calls",
            "reasoning",
            "include",
            "prompt_cache_key",
            "max_output_tokens",
        ],
    )?;
    let ignored_parameters = token_limits(&v, &["max_output_tokens"])?;
    let mut b = base(&v)?;
    let mut input = if let Some(s) = v["input"].as_str() {
        json!([{"role":"user","content":s}])
    } else {
        v["input"].clone()
    };
    let items = input
        .as_array_mut()
        .filter(|a| !a.is_empty())
        .ok_or_else(|| invalid("input"))?;
    let mut calls = HashSet::new();
    let mut pending = HashSet::new();
    for item in items {
        match item["type"].as_str().unwrap_or("message") {
            "message" => {
                fields(item, &["type", "role", "content", "id", "status"])?;
                if !["user", "assistant", "developer", "system"]
                    .contains(&item["role"].as_str().unwrap_or(""))
                {
                    return Err(invalid("input.role"));
                }
                let allows_images = item["role"] == "user";
                if let Some(parts) = item["content"].as_array_mut() {
                    for part in parts {
                        if part["type"] == "input_image" {
                            if !allows_images {
                                return Err(invalid("input.content"));
                            }
                            *part = input_image(part)?;
                            continue;
                        }
                        if !["input_text", "output_text", "refusal"]
                            .contains(&part["type"].as_str().unwrap_or(""))
                        {
                            return Err(invalid("input.content"));
                        }
                        if part["type"] == "refusal" {
                            string(part, "refusal")?;
                        } else if !part["text"].is_string() {
                            return Err(invalid("input.text"));
                        }
                    }
                } else if !item["content"].is_string() {
                    return Err(invalid("input.content"));
                }
                if !pending.is_empty() {
                    return Err(invalid("input.tool_history"));
                }
            }
            "function_call" => {
                fields(
                    item,
                    &["type", "id", "call_id", "name", "arguments", "status"],
                )?;
                let id = string(item, "call_id")?;
                if !calls.insert(id.to_owned()) || !name(string(item, "name")?) {
                    return Err(invalid("input.function_call"));
                }
                args(item, "arguments")?;
                pending.insert(id.to_owned());
            }
            "function_call_output" => {
                fields(item, &["type", "id", "call_id", "output", "status"])?;
                if !pending.remove(string(item, "call_id")?) || !item["output"].is_string() {
                    return Err(invalid("input.function_call_output"));
                }
            }
            "reasoning" => {
                fields(
                    item,
                    &[
                        "type",
                        "id",
                        "summary",
                        "content",
                        "encrypted_content",
                        "status",
                    ],
                )?;
                if let Some(status) = item.get("status").filter(|v| !v.is_null())
                    && !status
                        .as_str()
                        .is_some_and(|s| ["in_progress", "completed", "incomplete"].contains(&s))
                {
                    return Err(invalid("input.reasoning.status"));
                }
            }
            _ => return Err(invalid("input.type")),
        }
    }
    if !pending.is_empty() {
        return Err(invalid("input.tool_history"));
    }
    b.insert("input".into(), input);
    if let Some(i) = v.get("instructions") {
        if !i.is_string() {
            return Err(invalid("instructions"));
        }
        b.insert("instructions".into(), i.clone());
    }
    if let Some(r) = v.get_mut("reasoning") {
        omit_nulls(r, &["effort", "summary"]);
        fields(r, &["effort", "summary"])?;
        for key in ["effort", "summary"] {
            if r.get(key).is_some_and(|v| !v.is_string()) {
                return Err(invalid("reasoning"));
            }
        }
        b.insert("reasoning".into(), r.clone());
    }
    if let Some(include) = v.get("include") {
        if !include
            .as_array()
            .is_some_and(|a| a.iter().all(|i| i == "reasoning.encrypted_content"))
        {
            return Err(invalid("include"));
        }
        b.insert("include".into(), include.clone());
    }
    tools(&v, false, &mut b)?;
    Ok(Request {
        body: Value::Object(b),
        stream: boolean(&v, "stream", false)?,
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
                                args(item, "arguments")?;
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
