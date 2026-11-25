//! The actions as the Mac app reads them: `fixtures/actions.json` holds one of each, written exactly as this crate
//! sends it, and the app's `AgentGateTests` parses the same file with `AgentAction.parse`.

use client_mcp::{Action, Mode};
use serde_json::Value;

fn every_action() -> Vec<Action> {
    vec![
        Action::Connect,
        Action::Disconnect,
        Action::SetMode { mode: Mode::Global },
        Action::SelectNode {
            node: "Node A".to_string(),
            group: None,
        },
        Action::SelectNode {
            node: "Node A".to_string(),
            group: Some("Provider".to_string()),
        },
        Action::TestNodeDelays,
        Action::SetCapture { enabled: true },
        Action::ClearCapture,
        Action::CloseConnection { id: 7 },
    ]
}

#[test]
fn the_fixture_is_what_this_crate_sends() {
    let fixture: Vec<Value> = serde_json::from_str(include_str!("fixtures/actions.json")).unwrap();
    let sent: Vec<Value> = every_action()
        .iter()
        .map(|action| serde_json::to_value(action).unwrap())
        .collect();
    assert_eq!(sent, fixture);
    for (action, value) in every_action().iter().zip(&fixture) {
        assert_eq!(value["action"], action.kind(), "{value}");
        assert_eq!(
            &serde_json::from_value::<Action>(value.clone()).unwrap(),
            action
        );
    }
}
