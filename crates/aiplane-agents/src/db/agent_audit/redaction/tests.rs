use super::*;

fn sensitive(tools: &[&str]) -> Redaction {
    Redaction {
        secret: None,
        sensitive_tools: tools.iter().map(|t| t.to_string()).collect(),
    }
}

#[test]
fn a_sensitive_tools_arguments_are_redacted_in_its_result_and_its_gate() {
    let r = sensitive(&["otp_submit"]);
    for kind in [AuditKind::ToolResult, AuditKind::ToolCall] {
        let mut detail = json!({ "tool": "otp_submit", "arguments": { "code": "4711" } });
        r.apply(kind, &mut detail);
        assert_eq!(detail["arguments"], redacted_arguments(), "{kind:?}");
        let mut other = json!({ "tool": "echo", "arguments": { "m": "hi" } });
        r.apply(kind, &mut other);
        assert_eq!(other["arguments"], json!({ "m": "hi" }));
    }
}

#[test]
fn an_exchange_has_every_call_to_a_sensitive_tool_redacted_wherever_it_sits() {
    let call = |name: &str| {
        json!({ "id": "c", "type": "function",
                    "function": { "name": name, "arguments": "{\"code\":\"4711\"}" } })
    };
    let mut detail = json!({
        "request": { "messages": [
            { "role": "assistant", "tool_calls": [call("otp_submit"), call("echo")] }
        ] },
        "request_delta": { "messages": [
            { "role": "assistant", "tool_calls": [call("otp_submit")] }
        ] },
        "response": { "tool_calls": [call("otp_submit")] },
    });
    sensitive(&["otp_submit"]).apply(AuditKind::LlmExchange, &mut detail);
    assert_eq!(
        detail["request"]["messages"][0]["tool_calls"][0]["function"]["arguments"],
        redacted_arguments()
    );
    assert_eq!(
        detail["request"]["messages"][0]["tool_calls"][1]["function"]["arguments"],
        json!("{\"code\":\"4711\"}"),
        "another tool's call is kept"
    );
    assert_eq!(
        detail["request_delta"]["messages"][0]["tool_calls"][0]["function"]["arguments"],
        redacted_arguments()
    );
    assert_eq!(
        detail["response"]["tool_calls"][0]["function"]["arguments"],
        redacted_arguments()
    );
}

#[test]
fn a_secure_input_is_withheld_from_any_kind_of_event() {
    let r = Redaction::decided(
        SuspensionKind::SecureInput,
        &Decision::Value {
            value: json!(" 481516 "),
        },
    );
    for kind in AuditKind::ALL {
        let mut detail = json!({ "said": "the code is 481516", "n": 481516 });
        r.apply(*kind, &mut detail);
        assert!(!detail.to_string().contains("481516"), "{kind:?}: {detail}");
    }
    assert!(format!("{r:?}").contains("<withheld>"));
    assert!(!format!("{r:?}").contains("481516"));
}

#[test]
fn only_a_secure_input_is_a_secret() {
    let value = json!("yes");
    for kind in [SuspensionKind::Approval, SuspensionKind::HumanAnswer] {
        let decision = Decision::Value {
            value: value.clone(),
        };
        assert_eq!(Redaction::decided(kind, &decision).secret, None);
    }
    assert_eq!(
        Redaction::decided(SuspensionKind::SecureInput, &Decision::AllowOnce).secret,
        None
    );
}

#[test]
fn a_tool_that_repeats_a_secure_input_has_it_withheld() {
    use SECURE_INPUT_WITHHELD as W;
    let body = json!({"echo": "you typed 481516", "n": 481516, "ok": true});
    let withheld = json!({"echo": format!("you typed {W}"), "n": W, "ok": true});
    assert_eq!(withhold_secret(body.clone(), &json!("481516")), withheld);
    assert_eq!(
        withhold_secret(body.clone(), &json!(" 481516 ")),
        withheld,
        "typed with spaces around it, as the verifier trims it"
    );
    assert_eq!(
        withhold_secret(body.clone(), &json!("")),
        body,
        "an empty value matches nothing"
    );
}
