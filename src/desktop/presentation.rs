//! Pure presentation rules; all values come from the existing backend snapshots.

use tiny_proxy::gateway::Record;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GatewayAction {
    Start,
    Stop,
}

impl GatewayAction {
    pub(super) fn command(self, ip: &str, port: &str) -> Result<tiny_proxy::app::Command, String> {
        match self {
            Self::Start => {
                let ip = ip.trim();
                let ip = if ip.is_empty() { "127.0.0.1" } else { ip };
                let ip: std::net::IpAddr = ip
                    .parse()
                    .map_err(|_| "请输入具体的监听 IP，例如 127.0.0.1".to_owned())?;
                let port = port.trim();
                let port = if port.is_empty() { "8787" } else { port };
                if !port.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err("端口请输入 1–65535 的整数".into());
                }
                let port = port
                    .parse::<u16>()
                    .ok()
                    .filter(|port| *port != 0)
                    .ok_or_else(|| "端口请输入 1–65535 的整数".to_owned())?;
                let address = std::net::SocketAddr::new(ip, port);
                let address = tiny_proxy::gateway::validate_bind(&address.to_string())?;
                Ok(tiny_proxy::app::Command::StartGateway(address.to_string()))
            }
            // Stopping must not depend on the contents of editable inputs.
            Self::Stop => Ok(tiny_proxy::app::Command::StopGateway),
        }
    }
}

pub(super) fn gateway_bind_fields(address: &str) -> (String, String) {
    match address.parse::<std::net::SocketAddr>() {
        Ok(address) => (address.ip().to_string(), address.port().to_string()),
        Err(_) => (address.into(), String::new()),
    }
}

pub(super) struct GatewayControl {
    pub action: GatewayAction,
    pub label: &'static str,
    pub blocked: Option<&'static str>,
}

pub(super) fn gateway_control(
    running: bool,
    busy: bool,
    connected: bool,
    has_keys: bool,
    pending: Option<GatewayAction>,
) -> GatewayControl {
    if let Some(action) = pending {
        return GatewayControl {
            action,
            label: match action {
                GatewayAction::Start => "启动中…",
                GatewayAction::Stop => "停止中…",
            },
            blocked: Some("正在等待服务响应"),
        };
    }
    // Stopping an active listener must remain available during a model test.
    if running {
        return GatewayControl {
            action: GatewayAction::Stop,
            label: "停止服务",
            blocked: None,
        };
    }
    GatewayControl {
        action: GatewayAction::Start,
        label: "启动服务",
        blocked: if busy {
            Some("正在处理，请稍后")
        } else if !connected {
            Some("请先连接账号")
        } else if !has_keys {
            Some("请先创建客户端密钥")
        } else {
            None
        },
    }
}

pub(super) fn failure_label(code: &str) -> &'static str {
    match code {
        "upstream_stream_interrupted" => "流中断",
        "request_cancelled" | "client_disconnected" => "已取消",
        "upstream_timeout" => "上游超时",
        "slow_consumer" => "客户端读取超时",
        "upstream_rate_limit" => "额度或速率受限",
        "invalid_gateway_key" | "key_revoked" => "密钥无效",
        "unsupported_or_invalid_parameter" | "invalid_request" => "请求格式错误",
        _ => "请求失败",
    }
}

pub(super) fn valid_key_name(value: &str) -> bool {
    let value = value.trim();
    (1..=80).contains(&value.chars().count()) && !value.chars().any(char::is_control)
}

pub(super) fn short_time(value: i64) -> String {
    chrono::DateTime::from_timestamp_millis(value)
        .map(|t| {
            t.with_timezone(&chrono::Local)
                .format("%H:%M:%S%.3f")
                .to_string()
        })
        .unwrap_or_else(|| "未提供".into())
}

pub(super) fn request_summary(record: &Record) -> [(&'static str, String); 4] {
    [
        (
            "客户端 HTTP",
            record
                .http_status
                .map(|v| v.to_string())
                .unwrap_or_else(|| "未返回".into()),
        ),
        (
            "上游 HTTP",
            record
                .upstream_status
                .map(|v| v.to_string())
                .unwrap_or_else(|| "未提供".into()),
        ),
        (
            "流式",
            record
                .stream
                .map(|v| if v { "是" } else { "否" })
                .unwrap_or("未提供")
                .into(),
        ),
        (
            "输入 / 输出 tokens",
            format!(
                "{} / {}",
                record
                    .input_tokens
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "未提供".into()),
                record
                    .output_tokens
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "未提供".into())
            ),
        ),
    ]
}

pub(super) fn quota_fraction(percent: Option<f64>) -> Option<f32> {
    percent
        .filter(|p| p.is_finite() && (0. ..=100.).contains(p))
        .map(|p| p as f32 / 100.)
}

pub(super) fn matches_query(query: &str, fields: &[&str]) -> bool {
    let text = fields.join(" ").to_lowercase();
    query
        .split_whitespace()
        .all(|part| text.contains(&part.to_lowercase()))
}

#[cfg(test)]
pub(super) fn live_generations(records: &[Record]) -> usize {
    records
        .iter()
        .filter(|r| {
            r.ended_at.is_none()
                && r.method == "POST"
                && matches!(r.route.as_str(), "/v1/responses" | "/v1/chat/completions")
        })
        .count()
}

pub(super) fn needs_stream_warning(record: &Record) -> bool {
    record.stream == Some(true)
        && record.http_status == Some(200)
        && record.ended_at.is_some()
        && record.outcome != "completed"
}

pub(super) fn timeline(record: &Record) -> Vec<(&'static str, i64)> {
    let mut entries = vec![("请求开始", record.started_at)];
    for (label, time) in [
        ("首批上游数据", record.first_byte_at),
        ("首个输出", record.first_output_at),
        ("最近上游活动", record.last_activity_at),
    ] {
        if let Some(time) = time {
            entries.push((label, time));
        }
    }
    if let Some(time) = record.ended_at {
        entries.push((
            match record.outcome.as_str() {
                "interrupted" => "恢复确认（非真实结束）",
                "completed" => "完整结束",
                "cancelled" => "已取消",
                "incomplete" => "受限结束",
                _ => "请求失败",
            },
            time,
        ));
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_proxy::gateway::Record;

    #[test]
    fn key_name_validation_counts_characters_and_rejects_blank_or_oversized_names() {
        assert!(!valid_key_name("  "));
        assert!(valid_key_name(" 工作电脑 "));
        assert!(valid_key_name(&"中".repeat(80)));
        assert!(!valid_key_name(&"中".repeat(81)));
        assert!(!valid_key_name("work\nstation"));
    }

    #[test]
    fn service_can_start_again_after_stopping_using_the_same_bind_address() {
        let control = gateway_control(true, false, true, true, None);
        assert_eq!(control.label, "停止服务");
        assert!(control.blocked.is_none());
        assert!(matches!(
            control.action.command("ignored", "invalid"),
            Ok(tiny_proxy::app::Command::StopGateway)
        ));

        let control = gateway_control(false, false, true, true, None);
        assert_eq!(control.label, "启动服务");
        assert!(control.blocked.is_none());
        assert!(matches!(
            control.action.command(" 192.168.1.10 ", " 8877 "),
            Ok(tiny_proxy::app::Command::StartGateway(address)) if address == "192.168.1.10:8877"
        ));
        assert!(matches!(
            control.action.command("  ", "  "),
            Ok(tiny_proxy::app::Command::StartGateway(address)) if address == "127.0.0.1:8787"
        ));
    }

    #[test]
    fn changing_only_the_port_preserves_the_selected_ip() {
        for host in ["127.0.0.1", "192.168.1.10", "100.64.1.20", "::1", "fd00::1"] {
            for port in ["1", "8877", "65535"] {
                let command = GatewayAction::Start.command(host, port).unwrap();
                let tiny_proxy::app::Command::StartGateway(address) = command else {
                    panic!("expected a start command");
                };
                let address: std::net::SocketAddr = address.parse().unwrap();
                assert_eq!(address.ip(), host.parse::<std::net::IpAddr>().unwrap());
                assert_eq!(address.port(), port.parse::<u16>().unwrap());
            }
        }
        assert!(matches!(
            GatewayAction::Start.command("", "9000"),
            Ok(tiny_proxy::app::Command::StartGateway(address)) if address == "127.0.0.1:9000"
        ));
        assert!(matches!(
            GatewayAction::Start.command("192.168.1.10", ""),
            Ok(tiny_proxy::app::Command::StartGateway(address)) if address == "192.168.1.10:8787"
        ));
    }

    #[test]
    fn invalid_ports_report_the_range_and_do_not_create_a_start_command() {
        for port in [
            "0",
            "65536",
            "-1",
            "+8787",
            "8.7",
            "abc",
            "87 87",
            "８７８７",
        ] {
            assert_eq!(
                GatewayAction::Start
                    .command("127.0.0.1", port)
                    .err()
                    .as_deref(),
                Some("端口请输入 1–65535 的整数")
            );
        }
        for host in ["0.0.0.0", "8.8.8.8", "::", "localhost", "127.0.0.1:8787"] {
            assert!(GatewayAction::Start.command(host, "8877").is_err());
        }
    }

    #[test]
    fn restoring_saved_addresses_populates_both_inputs_without_changing_the_port() {
        for (address, host, port) in [
            ("127.0.0.1:9000", "127.0.0.1", "9000"),
            ("192.168.1.10:65535", "192.168.1.10", "65535"),
            ("[::1]:8877", "::1", "8877"),
            ("[fd00::1]:1234", "fd00::1", "1234"),
        ] {
            assert_eq!(gateway_bind_fields(address), (host.into(), port.into()));
            assert!(matches!(
                GatewayAction::Start.command(host, port),
                Ok(tiny_proxy::app::Command::StartGateway(value)) if value == address
            ));
        }
        // Corrupt saved settings remain visible for correction rather than
        // silently selecting a different listener.
        assert_eq!(gateway_bind_fields("broken"), ("broken".into(), "".into()));
        assert_eq!(
            gateway_bind_fields("127.0.0.1:0"),
            ("127.0.0.1".into(), "0".into())
        );
    }

    #[test]
    fn start_requires_ready_credentials_and_keys_but_stop_remains_available() {
        assert_eq!(
            gateway_control(false, false, false, true, None).blocked,
            Some("请先连接账号")
        );
        assert_eq!(
            gateway_control(false, false, true, false, None).blocked,
            Some("请先创建客户端密钥")
        );
        assert!(
            gateway_control(false, true, true, true, None)
                .blocked
                .is_some()
        );
        let control = gateway_control(true, true, false, false, None);
        assert_eq!(control.action, GatewayAction::Stop);
        assert!(control.blocked.is_none());
    }

    #[test]
    fn service_transitions_disable_repeat_clicks_until_the_backend_responds() {
        for (running, action, label) in [
            (false, GatewayAction::Start, "启动中…"),
            (true, GatewayAction::Stop, "停止中…"),
        ] {
            let control = gateway_control(running, false, true, true, Some(action));
            assert_eq!(control.label, label);
            assert!(control.blocked.is_some());
            assert!(
                gateway_control(running, false, true, true, None)
                    .blocked
                    .is_none()
            );
        }
    }

    #[test]
    fn unknown_or_invalid_quota_never_becomes_a_zero_or_full_progress_bar() {
        for value in [
            None,
            Some(f64::NAN),
            Some(f64::INFINITY),
            Some(-1.),
            Some(101.),
        ] {
            assert_eq!(quota_fraction(value), None);
        }
        assert_eq!(quota_fraction(Some(0.)), Some(0.));
        assert_eq!(quota_fraction(Some(74.)), Some(0.74));
        assert_eq!(quota_fraction(Some(100.)), Some(1.));
    }

    #[test]
    fn search_matches_metadata_case_insensitively_without_needing_bodies() {
        assert!(matches_query(
            " GPT-6 笔记本 ",
            &["gpt-6.1-sol", "Trae · 笔记本"]
        ));
        assert!(!matches_query(
            "gpt-6 工作站",
            &["gpt-6.1-sol", "Trae · 笔记本"]
        ));
        assert!(matches_query("  ", &["未提供"]));
    }

    #[test]
    fn live_generation_count_excludes_model_queries_and_finished_records() {
        let active = Record::new("active".into(), "POST".into(), "/v1/responses".into(), 1);
        let mut finished = active.clone();
        finished.ended_at = Some(2);
        let models = Record::new("models".into(), "GET".into(), "/v1/models".into(), 1);
        assert_eq!(live_generations(&[active, finished, models]), 1);
    }

    #[test]
    fn timeline_does_not_invent_output_or_use_recovery_as_actual_end() {
        let mut record = Record::new(
            "fixture".into(),
            "POST".into(),
            "/v1/responses".into(),
            1_000,
        );
        assert_eq!(timeline(&record), vec![("请求开始", 1_000)]);
        record.first_byte_at = Some(1_100);
        record.outcome = "interrupted".into();
        record.ended_at = Some(90_000);
        let entries = timeline(&record);
        assert_eq!(entries.last(), Some(&("恢复确认（非真实结束）", 90_000)));
        assert!(!entries.iter().any(|(label, _)| *label == "首个输出"));
    }

    #[test]
    fn http_200_is_not_a_successful_stream_terminal() {
        let mut record = Record::new("fixture".into(), "POST".into(), "/v1/responses".into(), 1);
        record.stream = Some(true);
        record.http_status = Some(200);
        record.ended_at = Some(2);
        record.outcome = "failed".into();
        assert!(needs_stream_warning(&record));
        record.outcome = "completed".into();
        assert!(!needs_stream_warning(&record));
        record.outcome = "failed".into();
        record.stream = Some(false);
        assert!(!needs_stream_warning(&record));
    }
}
