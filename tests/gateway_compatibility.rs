use axum::{
    Json, Router,
    routing::{get, post},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tiny_proxy::{
    auth::codex::{Service, now},
    gateway::Gateway,
    platform::protection,
    protocol::gateway::{normalize_chat, normalize_responses},
    storage::Store,
};

fn history() -> Value {
    json!({"model":"fixture","messages":[
        {"role":"user","content":"Read two files."},
        {"role":"assistant","content":null,"tool_calls":[
            {"id":"c1","type":"function","function":{"name":"read_file","arguments":"{\"path\": \"one\"}"}},
            {"id":"c2","type":"function","function":{"name":"read_file","arguments":"{\"path\": \"two\"}"}}
        ]},
        {"role":"tool","tool_call_id":"c2","content":"second"},
        {"role":"tool","tool_call_id":"c1","content":"first"}
    ]})
}

#[test]
fn streamed_call_indices_are_metadata_and_never_change_order_or_tool_identity() {
    let expected = normalize_chat(history()).unwrap().body;
    for indices in [
        [json!(0), json!(1)],
        [json!(9), json!(2)],
        [Value::Null, json!(0)],
    ] {
        let mut body = history();
        for (call, index) in body["messages"][1]["tool_calls"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .zip(indices)
        {
            call["index"] = index;
        }
        let actual = normalize_chat(body).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(actual.body, expected);
        assert!(actual.ignored_parameters.is_empty());
    }
}

#[test]
fn tool_result_names_and_assistant_empty_response_fields_roundtrip() {
    let expected = normalize_chat(history()).unwrap().body;
    let mut body = history();
    body["messages"][1]["refusal"] = Value::Null;
    body["messages"][1]["audio"] = Value::Null;
    body["messages"][1]["function_call"] = Value::Null;
    body["messages"][1]["annotations"] = json!([]);
    body["messages"][2]["name"] = json!("read_file");
    body["messages"][3]["name"] = Value::Null;
    assert_eq!(normalize_chat(body).unwrap().body, expected);
    let actual = normalize_chat(json!({"model":"fixture","messages":[
        {"role":"assistant","content":"hello","tool_calls":null,"refusal":null},
        {"role":"user","content":"continue"}
    ]}))
    .unwrap();
    assert_eq!(actual.body["input"][0]["content"][0]["text"], "hello");
}

#[test]
fn assistant_refusal_is_preserved_as_native_content_instead_of_discarded() {
    let actual = normalize_chat(json!({"model":"fixture","messages":[
        {"role":"assistant","content":null,"refusal":"Cannot help with that.","tool_calls":null},
        {"role":"user","content":"Try this instead."}
    ]}))
    .unwrap();
    assert_eq!(
        actual.body["input"][0],
        json!({"type":"message","role":"assistant","content":[
            {"type":"refusal","refusal":"Cannot help with that."}
        ]})
    );
}

#[test]
fn optional_nulls_and_unknown_fields_are_accepted_but_model_is_required() {
    let streaming = normalize_chat(json!({"model":"fixture","stream":true,
        "stream_options":{"include_usage":null},"messages":[{"role":"user","content":"hi"}]}))
    .unwrap();
    assert!(streaming.stream);
    assert!(!streaming.include_usage);
    for chat in [true, false] {
        let base = if chat {
            json!({"model":"fixture","messages":[{"role":"user","content":"hi"}]})
        } else {
            json!({"model":"fixture","input":"hi"})
        };
        let normalize = if chat {
            normalize_chat
        } else {
            normalize_responses
        };
        let expected = normalize(base.clone()).unwrap().body;
        let common = [
            "stream",
            "store",
            "tools",
            "tool_choice",
            "parallel_tool_calls",
            "prompt_cache_key",
            "temperature",
            "top_p",
            "metadata",
            "user",
            "service_tier",
        ];
        let extra = if chat {
            vec![
                "stream_options",
                "reasoning_effort",
                "n",
                "stop",
                "seed",
                "logprobs",
                "top_logprobs",
                "frequency_penalty",
                "presence_penalty",
                "logit_bias",
                "response_format",
            ]
        } else {
            vec!["instructions", "reasoning", "include", "text", "truncation"]
        };
        for field in common.into_iter().chain(extra) {
            let mut body = base.clone();
            body[field] = Value::Null;
            let actual = normalize(body).unwrap_or_else(|e| panic!("chat={chat} {field}: {e}"));
            assert_eq!(actual.body, expected, "chat={chat} {field}");
            assert!(actual.ignored_parameters.is_empty());
        }
        for (field, value) in [
            ("unexpected", Value::Null),
            ("model", Value::Null),
            ("temperature", json!(0.2)),
            ("store", json!(true)),
            ("tool_choice", json!("made-up")),
            ("parallel_tool_calls", json!("false")),
        ] {
            let mut body = base.clone();
            body[field] = value;
            if field == "model" {
                assert!(normalize(body).is_err(), "model is required");
            } else {
                assert!(normalize(body).is_ok(), "must accept {field}");
            }
        }
    }
}

#[test]
fn empty_function_schema_and_nullable_tool_options_work_for_both_protocols() {
    for chat in [true, false] {
        let mut body = if chat {
            json!({"model":"fixture","messages":[{"role":"user","content":"hi"}]})
        } else {
            json!({"model":"fixture","input":"hi"})
        };
        body["tools"] = if chat {
            json!([{"type":"function","function":{"name":"ping","description":null,"strict":null}}])
        } else {
            json!([{"type":"function","name":"ping","description":null,"strict":null}])
        };
        body["tool_choice"] = json!("required");
        let normalize = if chat {
            normalize_chat
        } else {
            normalize_responses
        };
        let actual = normalize(body.clone()).unwrap();
        assert_eq!(
            actual.body["tools"],
            json!([{"type":"function","name":"ping","parameters":{"type":"object","properties":{}}}])
        );
        let function = if chat {
            &mut body["tools"][0]["function"]
        } else {
            &mut body["tools"][0]
        };
        function["strict"] = json!(false);
        function["description"] = json!("Ping");
        function["parameters"] = json!({"type":"object","properties":{"nested":{"const":{"index":7,"status":"keep","default":null}}}});
        let actual = normalize(body.clone()).unwrap();
        assert_eq!(actual.body["tools"][0]["strict"], false);
        assert_eq!(
            actual.body["tools"][0]["parameters"]["properties"]["nested"]["const"],
            json!({"index":7,"status":"keep","default":null})
        );
        for (key, value) in [
            ("parameters", Value::Null),
            ("parameters", json!([])),
            ("strict", json!("false")),
            ("description", json!(5)),
            ("unexpected", Value::Null),
        ] {
            let mut invalid = body.clone();
            let function = if chat {
                &mut invalid["tools"][0]["function"]
            } else {
                &mut invalid["tools"][0]
            };
            function[key] = value.clone();
            let actual = normalize(invalid).unwrap();
            assert_eq!(actual.body["tools"][0][key], value, "chat={chat} {key}");
        }
    }
}

#[test]
fn responses_reasoning_replay_preserves_valid_status_and_opaque_payloads() {
    let item = json!({"type":"reasoning","id":"r1","summary":[],"encrypted_content":"synthetic-ciphertext"});
    let mut with_status = item.clone();
    with_status["status"] = json!("completed");
    let actual = normalize_responses(json!({"model":"fixture","input":[with_status.clone(),
        {"role":"user","content":"continue"}],"reasoning":{"effort":null,"summary":"auto"}}))
    .unwrap();
    assert_eq!(actual.body["input"][0], with_status);
    assert_eq!(actual.body["reasoning"], json!({"summary":"auto"}));
    for status in [json!("unknown"), json!(1), json!({})] {
        let mut invalid_item = item.clone();
        invalid_item["status"] = status.clone();
        let request =
            normalize_responses(json!({"model":"fixture","input":[invalid_item]})).unwrap();
        assert_eq!(request.body["input"][0]["status"], status);
    }
}

fn trusted_histories() -> Vec<Value> {
    let mut cases = vec![];
    for value in [json!(-1), json!(0.5), json!("0"), json!(true), json!({})] {
        let mut body = history();
        body["messages"][1]["tool_calls"][0]["index"] = value;
        cases.push(body);
    }
    for pointer in [
        "/index",
        "/messages/1/index",
        "/messages/1/tool_calls/0/function/index",
    ] {
        let mut body = history();
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        body.pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(key.into(), json!(0));
        cases.push(body);
    }
    for (index, field, value) in [
        (2, "name", json!("different_tool")),
        (1, "refusal", json!(true)),
        (0, "refusal", Value::Null),
        (0, "tool_calls", Value::Null),
        (1, "audio", json!({"id":"audio"})),
        (1, "annotations", json!([{"type":"url_citation"}])),
        (1, "function_call", json!({"name":"legacy"})),
        (1, "unknown", Value::Null),
        (2, "tool_call_id", json!("orphan")),
    ] {
        let mut body = history();
        body["messages"][index][field] = value;
        cases.push(body);
    }
    let mut body = history();
    body["messages"].as_array_mut().unwrap().pop();
    cases.push(body);
    let mut body = history();
    body["messages"][1]["tool_calls"][1]["id"] = json!("c1");
    cases.push(body);
    let mut body = history();
    body["messages"][1]["tool_calls"][0]["function"]["arguments"] = json!("partial{");
    cases.push(body);
    cases
}

#[test]
fn client_metadata_and_tool_history_are_left_for_upstream_validation() {
    for body in trusted_histories() {
        let expected_id = body["messages"][1]["tool_calls"][0]["id"].clone();
        let expected_args = body["messages"][1]["tool_calls"][0]["function"]["arguments"].clone();
        let request = normalize_chat(body).unwrap();
        assert_eq!(request.body["input"][1]["call_id"], expected_id);
        assert_eq!(request.body["input"][1]["arguments"], expected_args);
    }
}

#[tokio::test]
async fn http_client_can_replay_aggregated_stream_tools_then_continue_in_json_and_sse() {
    let seen = Arc::new(Mutex::new(Vec::<Value>::new()));
    let received = seen.clone();
    let upstream = Router::new()
        .route("/models", get(|| async { Json(json!({"models":[{"slug":"fixture"}]})) }))
        .route("/responses", post(move |Json(body): Json<Value>| {
            let continued = body["input"].as_array().unwrap().iter().any(|v| v["type"] == "function_call_output");
            received.lock().unwrap().push(body);
            async move {
                let output = if continued { json!([{"type":"message","role":"assistant","content":[{"type":"output_text","text":"continued"}]}]) }
                             else { json!([
                                 {"type":"function_call","id":"fc1","call_id":"c1","name":"read_file","arguments":"{\"path\": \"one\"}"},
                                 {"type":"function_call","id":"fc2","call_id":"c2","name":"read_file","arguments":"{\"path\": \"two\"}"}
                             ]) };
                let mut events = vec![json!({"type":"response.created","response":{"id":"r","model":"fixture","created_at":100,"status":"in_progress"}})];
                if !continued {
                    for (index, item) in output.as_array().unwrap().iter().enumerate() {
                        let mut start = item.clone(); start["arguments"] = json!("");
                        events.push(json!({"type":"response.output_item.added","output_index":index,"item":start}));
                        events.push(json!({"type":"response.function_call_arguments.delta","output_index":index,"delta":item["arguments"]}));
                        events.push(json!({"type":"response.output_item.done","output_index":index,"item":item}));
                    }
                }
                events.push(json!({"type":"response.completed","response":{"id":"r","model":"fixture","created_at":100,"status":"completed","output":output}}));
                ([("content-type","text/event-stream")], events.iter().map(|e| format!("data: {e}\n\n")).collect::<String>())
            }
        }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("app.db")).await.unwrap();
    let bundle = json!({"access_token":"fixture","refresh_token":"fixture","account_id":"fixture","subject":"fixture","expires_at":now()+3600});
    store
        .save_codex_credentials(
            0,
            &protection::protect(bundle.to_string().as_bytes()).unwrap(),
            now() + 3600,
        )
        .await
        .unwrap();
    let service = Service::for_test(store.clone(), address).unwrap();
    let key = store.create_key("compatibility fixture").await.unwrap();
    let mut gateway = Gateway::start("127.0.0.1:0".parse().unwrap(), store, service)
        .await
        .unwrap();
    let url = format!("http://{}/v1/chat/completions", gateway.address);
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap();
    for first_stream in [false, true] {
        let response = http.post(&url).bearer_auth(key.value.as_str()).json(&json!({
            "model":"fixture","stream":first_stream,"messages":[{"role":"user","content":"Read two files."}]
        })).send().await.unwrap();
        assert_eq!(response.status(), 200);
        let mut message = if first_stream {
            let text = response.text().await.unwrap();
            assert!(text.ends_with("data: [DONE]\n\n"));
            let mut calls = std::collections::BTreeMap::<u64, Value>::new();
            for line in text
                .lines()
                .filter_map(|l| l.strip_prefix("data: "))
                .filter(|l| *l != "[DONE]")
            {
                let chunk: Value = serde_json::from_str(line).unwrap();
                if let Some(deltas) = chunk["choices"][0]["delta"]["tool_calls"].as_array() {
                    for delta in deltas {
                        let index = delta["index"].as_u64().unwrap();
                        if delta.get("id").is_some() {
                            calls.insert(index, delta.clone());
                        } else {
                            let args = format!(
                                "{}{}",
                                calls[&index]["function"]["arguments"].as_str().unwrap(),
                                delta["function"]["arguments"].as_str().unwrap()
                            );
                            calls.get_mut(&index).unwrap()["function"]["arguments"] = json!(args);
                        }
                    }
                }
            }
            json!({"role":"assistant","content":null,"tool_calls":calls.into_values().collect::<Vec<_>>()})
        } else {
            response.json::<Value>().await.unwrap()["choices"][0]["message"].clone()
        };
        assert_eq!(message["tool_calls"].as_array().unwrap().len(), 2);
        message["refusal"] = Value::Null;
        message["reasoning_content"] = json!("synthetic-thinking");
        for stream in [false, true] {
            let mut body = history();
            body["messages"][1] = message.clone();
            body["stream"] = json!(stream);
            body["messages"][2]["name"] = json!("read_file");
            body["max_tokens"] = json!(16);
            body["temperature"] = Value::Null;
            let response = http
                .post(&url)
                .bearer_auth(key.value.as_str())
                .json(&body)
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            assert_eq!(
                response.headers()["x-tiny-proxy-ignored-parameters"],
                "max_tokens, messages[].reasoning_content"
            );
            if stream {
                assert!(response.text().await.unwrap().ends_with("data: [DONE]\n\n"));
            } else {
                assert_eq!(
                    response.json::<Value>().await.unwrap()["choices"][0]["message"]["content"],
                    "continued"
                );
            }
            assert_eq!(
                *seen.lock().unwrap().last().unwrap(),
                normalize_chat(history()).unwrap().body
            );
        }
    }
    let count = seen.lock().unwrap().len();
    assert_eq!(count, 6);
    for body in trusted_histories() {
        let response = http
            .post(&url)
            .bearer_auth(key.value.as_str())
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(
            response.json::<Value>().await.unwrap()["choices"][0]["message"]["content"],
            "continued"
        );
    }
    assert_eq!(
        seen.lock().unwrap().len(),
        count + trusted_histories().len()
    );
    gateway.stop().await;
    task.abort();
}
