use serde_json::json;
use tiny_proxy::protocol::{
    responses::{Error, TextProbe},
    sse::Event,
};
fn event(value: serde_json::Value) -> Event {
    Event {
        event: "message".into(),
        data: value.to_string(),
    }
}

#[test]
fn text_and_item_done_do_not_turn_eof_into_success() {
    let mut probe = TextProbe::new(1024);
    probe
        .accept(event(
            json!({"type":"response.output_text.delta","delta":"hello"}),
        ))
        .unwrap();
    probe
        .accept(event(
            json!({"type":"response.output_item.done","item":{"type":"message"}}),
        ))
        .unwrap();
    assert!(matches!(probe.finish(), Err(Error::Interrupted)));
}

#[test]
fn only_matching_completed_response_is_success() {
    let mut probe = TextProbe::new(1024);
    probe
        .accept(event(
            json!({"type":"response.created","response":{"id":"resp_test","status":"in_progress"}}),
        ))
        .unwrap();
    probe
        .accept(event(
            json!({"type":"response.output_text.delta","delta":"你好"}),
        ))
        .unwrap();
    probe.accept(event(json!({"type":"response.completed","response":{"id":"resp_test","status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"你好"}]}],"usage":{"input_tokens":2,"output_tokens":1}}}))).unwrap();
    let (text, response) = probe.finish().unwrap();
    assert_eq!(text, "你好");
    assert_eq!(response["usage"]["input_tokens"], 2);
}

#[test]
fn failed_incomplete_and_mismatched_events_never_succeed() {
    for (kind, expected) in [
        ("response.failed", Error::Failed),
        ("response.incomplete", Error::Incomplete),
    ] {
        let mut probe = TextProbe::new(1024);
        assert_eq!(
            probe.accept(event(json!({"type":kind,"response":{}}))),
            Err(expected)
        );
        assert!(probe.finish().is_err());
    }
    let mut probe = TextProbe::new(1024);
    assert_eq!(
        probe.accept(Event {
            event: "response.completed".into(),
            data: json!({"type":"response.failed"}).to_string()
        }),
        Err(Error::Protocol)
    );
}

#[test]
fn duplicate_terminal_output_mismatch_and_output_budget_are_errors() {
    let mut probe = TextProbe::new(1024);
    let completed = json!({"type":"response.completed","response":{"id":"resp_test","status":"completed","output":[]}});
    probe.accept(event(completed.clone())).unwrap();
    assert_eq!(probe.accept(event(completed)), Err(Error::Protocol));
    let mut probe = TextProbe::new(4);
    assert_eq!(
        probe.accept(event(
            json!({"type":"response.output_text.delta","delta":"你好"})
        )),
        Err(Error::TooLarge)
    );
    let mut probe = TextProbe::new(1024);
    probe
        .accept(event(
            json!({"type":"response.output_text.delta","delta":"a"}),
        ))
        .unwrap();
    assert_eq!(probe.accept(event(json!({"type":"response.completed","response":{"id":"resp_test","status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"b"}]}]}}))), Err(Error::Protocol));
}
