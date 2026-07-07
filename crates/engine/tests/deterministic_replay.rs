use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};

use engine::{
    matching::MatchingEngine,
    replay::{parse_jsonl, ReplayDriver, ReplayEventKind},
};

fn workspace_path(relative_path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative_path)
}

fn assert_replay_matches_golden(input_path: &str, expected_path: &str) {
    let input_path = workspace_path(input_path);
    let expected_path = workspace_path(expected_path);
    let input = fs::read_to_string(&input_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", input_path.display()));
    let events = parse_jsonl(Cursor::new(input.as_bytes()))
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", input_path.display()));
    let symbol = match &events
        .first()
        .unwrap_or_else(|| panic!("scenario {} is empty", input_path.display()))
        .kind
    {
        ReplayEventKind::NewOrder { order } => order.symbol.clone(),
        ReplayEventKind::Cancel { symbol, .. } => symbol.clone(),
    };

    let mut driver = ReplayDriver::new(MatchingEngine::new(symbol));
    let result = driver
        .replay_events(events)
        .unwrap_or_else(|error| panic!("failed to replay {}: {error}", input_path.display()));
    let mut actual = result
        .outputs
        .iter()
        .map(|event| serde_json::to_string(event).expect("replay output must serialize"))
        .collect::<Vec<_>>()
        .join("\n");
    if !actual.is_empty() {
        actual.push('\n');
    }

    let expected = fs::read_to_string(&expected_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", expected_path.display()));

    assert_eq!(
        actual,
        expected,
        "replay output for {} did not match {}",
        input_path.display(),
        expected_path.display()
    );
}

#[test]
fn basic_cross_matches_golden() {
    assert_replay_matches_golden(
        "data/scenarios/basic_cross.jsonl",
        "data/expected/basic_cross.out.jsonl",
    );
}

#[test]
fn partial_fills_match_golden() {
    assert_replay_matches_golden(
        "data/scenarios/partial_fills.jsonl",
        "data/expected/partial_fills.out.jsonl",
    );
}

#[test]
fn cancel_resting_order_matches_golden() {
    assert_replay_matches_golden(
        "data/scenarios/cancels.jsonl",
        "data/expected/cancels.out.jsonl",
    );
}
