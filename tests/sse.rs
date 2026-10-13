use tiny_proxy::protocol::sse::{Decoder, Error, Event};

#[test]
fn every_byte_boundary_preserves_utf8_crlf_and_multiline_data() {
    let wire = "\u{feff}: heartbeat\r\nevent: response.output_text.delta\r\ndata: 你好\r\ndata: world\r\n\r\ndata: next\n\n".as_bytes();
    let expected = vec![
        Event {
            event: "response.output_text.delta".into(),
            data: "你好\nworld".into(),
        },
        Event {
            event: "message".into(),
            data: "next".into(),
        },
    ];
    for split in 0..=wire.len() {
        let mut decoder = Decoder::new(1024);
        let mut events = decoder.push(&wire[..split]).unwrap();
        events.extend(decoder.push(&wire[split..]).unwrap());
        decoder.finish().unwrap();
        assert_eq!(events, expected, "split={split}");
    }
    let mut decoder = Decoder::new(1024);
    let events = wire
        .iter()
        .flat_map(|b| decoder.push(&[*b]).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(events, expected);
}

#[test]
fn cr_only_empty_data_and_comments_follow_sse_framing() {
    let mut decoder = Decoder::new(1024);
    assert_eq!(
        decoder.push(b": ping\r\rdata:\r\rdata: value\r\r").unwrap(),
        vec![
            Event {
                event: "message".into(),
                data: "".into()
            },
            Event {
                event: "message".into(),
                data: "value".into()
            },
        ]
    );
    decoder.finish().unwrap();
}

#[test]
fn incomplete_frames_and_invalid_utf8_are_errors() {
    let mut decoder = Decoder::new(1024);
    decoder.push(b"data: partial\n").unwrap();
    assert_eq!(decoder.finish(), Err(Error::Truncated));
    let mut decoder = Decoder::new(1024);
    assert_eq!(decoder.push(b"data: \xff\n\n"), Err(Error::InvalidUtf8));
}

#[test]
fn frame_budget_is_bounded_even_for_comments_and_many_data_lines() {
    let mut decoder = Decoder::new(24);
    assert_eq!(
        decoder.push(b": abcdefghijklmnopqrstuvwxyz"),
        Err(Error::TooLarge)
    );
    let mut decoder = Decoder::new(24);
    decoder.push(b"data: a\ndata: b\n").unwrap();
    assert_eq!(decoder.push(b"data: c\ndata: d\n\n"), Err(Error::TooLarge));
}

#[test]
fn budget_resets_between_events_not_between_network_chunks() {
    let mut decoder = Decoder::new(12);
    let frames = decoder.push(b"data: a\n\ndata: b\n\ndata: c\n\n").unwrap();
    assert_eq!(frames.len(), 3);
}
