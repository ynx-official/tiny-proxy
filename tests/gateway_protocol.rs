use serde_json::{Value, json};
use tiny_proxy::protocol::gateway::{Projection, normalize_chat, normalize_responses};

#[test]
fn chat_history_converts_instructions_and_tool_roundtrip_without_changing_arguments() {
    let args = "{\"path\": \"src/main.rs\"}";
    let r = normalize_chat(json!({"model":"test-model","stream":true,"messages":[
        {"role":"system","content":"You write code."},
        {"role":"user","content":"read the file"},
        {"role":"assistant","content":null,"tool_calls":[{"id":"call_1","type":"function","function":{"name":"read_file","arguments":args}}]},
        {"role":"tool","tool_call_id":"call_1","content":"hello"}
    ],"tools":[{"type":"function","function":{"name":"read_file","parameters":{"type":"object"},"strict":false}}]})).unwrap();
    assert_eq!(r.body["instructions"], "You write code.");
    assert_eq!(r.body["input"][1]["arguments"], args);
    assert_eq!(r.body["input"][2]["call_id"], "call_1");
    assert_eq!(r.body["tools"][0]["name"], "read_file");
    assert_eq!(r.body["store"], false);
}

#[test]
fn unused_fields_and_orphan_tool_outputs_are_accepted_for_upstream_validation() {
    assert!(
        normalize_chat(
            json!({"model":"m","messages":[{"role":"user","content":"hi"}],"temperature":0.2})
        )
        .is_ok()
    );
    assert!(normalize_chat(json!({"model":"m","messages":[{"role":"tool","tool_call_id":"unknown","content":"result"}]})).is_ok());
    assert!(
        normalize_responses(json!({"model":"m","input":"hi","previous_response_id":"old"})).is_ok()
    );
    assert!(normalize_chat(json!({"model":"m","messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"x"}}]}]})).is_err());
}

#[test]
fn token_limits_are_accepted_without_forwarding_them_to_codex() {
    for field in ["max_tokens", "max_completion_tokens", "max_output_tokens"] {
        for value in [json!(1), json!(32_000), Value::Null] {
            let mut body = if field == "max_output_tokens" {
                json!({"model":"m","input":"hi"})
            } else {
                json!({"model":"m","messages":[{"role":"user","content":"hi"}]})
            };
            body[field] = value;
            let request = if field == "max_output_tokens" {
                normalize_responses(body)
            } else {
                normalize_chat(body)
            }
            .unwrap_or_else(|error| panic!("{field}: {error}"));
            for omitted in ["max_tokens", "max_completion_tokens", "max_output_tokens"] {
                assert!(request.body.get(omitted).is_none());
            }
            assert_eq!(request.body["model"], "m");
            assert!(!request.body["input"].as_array().unwrap().is_empty());
        }
    }
}

#[test]
fn unused_token_limit_values_are_not_locally_validated() {
    for field in ["max_tokens", "max_completion_tokens", "max_output_tokens"] {
        for value in [
            json!(0),
            json!(-1),
            json!(1.5),
            json!("1024"),
            json!(true),
            json!([]),
            json!({}),
        ] {
            let mut body = if field == "max_output_tokens" {
                json!({"model":"m","input":"hi"})
            } else {
                json!({"model":"m","messages":[{"role":"user","content":"hi"}]})
            };
            body[field] = value;
            let result = if field == "max_output_tokens" {
                normalize_responses(body)
            } else {
                normalize_chat(body)
            };
            let request = result.expect("unused output limit must not block conversion");
            assert!(request.body.get(field).is_none());
            assert!(request.ignored_parameters.contains(&field));
        }
    }
}

#[test]
fn conversion_errors_locate_the_required_structure_without_echoing_values() {
    for (chat, body, path, rule) in [
        (
            true,
            json!({"model":null,"messages":[],"private-field":true}),
            "model",
            "非空字符串",
        ),
        (true, json!({"model":"m","messages":{}}), "messages", "数组"),
        (
            true,
            json!({"model":"m","messages":[{"role":"assistant","tool_calls":[{"id":"c1","function":{"arguments":"private arguments"}}]}]}),
            "messages[0].tool_calls[0].function.name",
            "非空字符串",
        ),
        (
            true,
            json!({"model":"m","messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"https://example.test/private-image"}}]}]}),
            "messages[0].content[0].image_url.url",
            "Base64",
        ),
        (
            true,
            json!({"model":"m","messages":[],"tools":{}}),
            "tools",
            "数组",
        ),
        (false, json!({"model":"m","input":42}), "input", "输入对象"),
    ] {
        let error = (if chat {
            normalize_chat(body)
        } else {
            normalize_responses(body)
        })
        .err()
        .expect("required structure must fail");
        assert_eq!(error.param, path);
        let diagnostic = error.diagnostic_message();
        assert!(diagnostic.contains(path));
        assert!(diagnostic.contains(rule));
        assert!(diagnostic.contains("建议"));
        assert!(!diagnostic.contains("private"));
    }
}

#[test]
fn unknown_fields_do_not_interfere_with_conversion_or_enter_chat_prompts() {
    for field in [
        "private-field-name",
        "temperature.private-token",
        "private\nsecret",
        "object",
    ] {
        let mut body = json!({"model":"m","messages":[{"role":"user","content":"prompt"}]});
        body["messages"][0][field] = json!("private field value");
        let request = normalize_chat(body).unwrap();
        assert_eq!(request.body["input"][0]["content"][0]["text"], "prompt");
        assert!(!request.body.to_string().contains("private"));
    }
}

fn created() -> Value {
    json!({"type":"response.created","response":{"id":"resp_1","created_at":1234,"model":"m","status":"in_progress"}})
}
fn call(index: u64, name: &str, args: &str) -> Value {
    json!({"id":format!("item_{index}"),"type":"function_call","call_id":format!("call_{index}"),"name":name,"arguments":args,"status":"completed"})
}

#[test]
fn trusting_client_arguments_does_not_relax_completed_upstream_tool_integrity() {
    let request = normalize_chat(
        json!({"model":"m","messages":[{"role":"assistant","tool_calls":[
            {"id":"c1","function":{"name":"read_file","arguments":"partial{"}}
        ]}]}),
    )
    .unwrap();
    assert_eq!(request.body["input"][0]["arguments"], "partial{");
    for arguments in ["partial{", "[]", "null"] {
        let mut projection = Projection::new("m".into(), false);
        projection.accept(&created()).unwrap();
        let terminal = json!({"type":"response.completed","response":{"id":"resp_1","created_at":1234,
            "model":"m","status":"completed","output":[call(0,"read_file",arguments)]}});
        assert!(
            projection.accept(&terminal).is_err(),
            "invalid completed tool arguments must fail"
        );
    }
}

#[test]
fn interleaved_tools_keep_dense_stable_indices_and_reconcile_final_arguments() {
    let mut p = Projection::new("m".into(), true);
    p.accept(&created()).unwrap();
    for i in [2, 4] {
        p.accept(&json!({"type":"response.output_item.added","output_index":i,"item":call(i,"read_file","")})).unwrap();
    }
    let delta = p.accept(&json!({"type":"response.function_call_arguments.delta","output_index":4,"delta":"{\"x\":"})).unwrap();
    assert_eq!(delta[0]["choices"][0]["delta"]["tool_calls"][0]["index"], 1);
    let final_response = json!({"id":"resp_1","created_at":1234,"model":"m","status":"completed","output":[call(2,"read_file","{}"),call(4,"read_file","{\"x\":1}")],"usage":{"input_tokens":10,"output_tokens":5,"total_tokens":15}});
    let chunks = p
        .accept(&json!({"type":"response.completed","response":final_response}))
        .unwrap();
    assert!(
        chunks
            .iter()
            .any(|v| v["choices"][0]["finish_reason"] == "tool_calls")
    );
    let out = p.finish().unwrap();
    assert_eq!(
        out["choices"][0]["message"]["tool_calls"][1]["function"]["arguments"],
        "{\"x\":1}"
    );
    assert_eq!(out["usage"]["prompt_tokens"], 10);
    assert_eq!(out["created"], 1234);
}

#[test]
fn truncated_or_failed_stream_is_never_success() {
    let mut p = Projection::new("m".into(), false);
    p.accept(&created()).unwrap();
    p.accept(&json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":"partial"})).unwrap();
    assert!(p.finish().is_err());
    let mut p = Projection::new("m".into(), false);
    p.accept(&created()).unwrap();
    assert!(p.accept(&json!({"type":"response.failed","response":{"id":"resp_1","status":"failed","error":{"message":"secret upstream details"}}})).is_err());
}

#[test]
fn completed_output_can_be_reconstructed_from_finished_items_only() {
    let item = json!({"id":"message_1","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Tiny Proxy connection OK"}]});
    let terminal = json!({"type":"response.completed","response":{"id":"resp_1","created_at":1234,"model":"m","status":"completed","output":[],"usage":{"input_tokens":4,"output_tokens":5}}});
    let mut p = Projection::new("m".into(), true);
    p.accept(&created()).unwrap();
    p.accept(&json!({"type":"response.output_item.added","output_index":0,"item":{"id":"message_1","type":"message","status":"in_progress","content":[]}})).unwrap();
    p.accept(&json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":"Tiny Proxy connection OK"})).unwrap();
    p.accept(&json!({"type":"response.output_item.done","output_index":0,"item":item}))
        .unwrap();
    assert!(
        p.finish().is_err(),
        "an item.done event alone is not a completed response"
    );
    p.accept(&terminal).unwrap();
    let output = p.finish().unwrap();
    assert_eq!(
        output["choices"][0]["message"]["content"],
        "Tiny Proxy connection OK"
    );
    assert_eq!(output["choices"][0]["finish_reason"], "stop");
    assert_eq!(p.terminal().unwrap()["output"][0], item);

    for unfinished_item in [false, true] {
        let mut p = Projection::new("m".into(), false);
        p.accept(&created()).unwrap();
        p.accept(&json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":"Tiny Proxy connection OK"})).unwrap();
        if unfinished_item {
            p.accept(&json!({"type":"response.output_item.done","output_index":0,"item":item}))
                .unwrap();
            p.accept(&json!({"type":"response.output_item.added","output_index":1,"item":{"id":"unfinished","type":"message","content":[]}})).unwrap();
        }
        assert!(
            p.accept(&terminal).is_err(),
            "missing finished item must not turn into success"
        );
    }
    let mut p = Projection::new("m".into(), false);
    p.accept(&created()).unwrap();
    p.accept(&json!({"type":"response.output_item.done","output_index":0,"item":item}))
        .unwrap();
    let mut mismatch = terminal.clone();
    mismatch["response"]["id"] = json!("another-response");
    assert!(p.accept(&mismatch).is_err());
}
