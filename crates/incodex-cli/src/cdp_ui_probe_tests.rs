use serde_json::{json, Value};

use super::{ui_ready_expression, validate_ui_probe_result};

fn runtime_evaluate_result(value: Value) -> Value {
    json!({
        "result": {
            "result": {
                "value": value
            }
        }
    })
}

#[test]
fn cdp_ui_probe_returns_separate_button_and_banner_fields() {
    let expression = ui_ready_expression();
    assert!(expression.contains("button"));
    assert!(expression.contains("banner"));
    assert!(validate_ui_probe_result(&runtime_evaluate_result(json!({
        "button": true,
        "banner": true
    })))
    .is_ok());
}

#[test]
fn cdp_ui_probe_distinguishes_each_missing_surface() {
    let cases = [
        (
            json!({ "button": false, "banner": true }),
            "Incodex button is not mounted yet",
        ),
        (
            json!({ "button": true, "banner": false }),
            "Incodex banner is not mounted yet",
        ),
        (
            json!({ "button": false, "banner": false }),
            "Incodex button and banner are not mounted yet",
        ),
    ];

    for (probe, expected) in cases {
        assert_eq!(
            validate_ui_probe_result(&runtime_evaluate_result(probe)).unwrap_err(),
            expected
        );
    }
}

#[test]
fn cdp_ui_probe_rejects_malformed_results_instead_of_accepting_them() {
    let malformed = [
        json!(true),
        json!(null),
        json!({}),
        json!({ "button": true }),
        json!({ "banner": true }),
        json!({ "button": "yes", "banner": true }),
        json!({ "button": true, "banner": 1 }),
    ];

    for probe in malformed {
        let error = validate_ui_probe_result(&runtime_evaluate_result(probe)).unwrap_err();
        assert!(
            error.contains("malformed Incodex UI probe result"),
            "unexpected error: {error}"
        );
    }

    assert!(validate_ui_probe_result(&json!({ "result": {} }))
        .unwrap_err()
        .contains("malformed Incodex UI probe result"));
}
