use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};

use engine::{
    matching::MatchingEngine,
    replay::{parse_jsonl, ReplayDriver, ReplayError, ReplayEvent, ReplayEventKind, ReplayResult},
};

fn workspace_path(relative_path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative_path)
}

fn load_scenario(input_path: &str) -> Vec<ReplayEvent> {
    let input_path = workspace_path(input_path);
    let input = fs::read_to_string(&input_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", input_path.display()));
    parse_jsonl(Cursor::new(input.as_bytes()))
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", input_path.display()))
}

fn replay_scenario(input_path: &str) -> (ReplayDriver, Result<ReplayResult, ReplayError>) {
    let events = load_scenario(input_path);
    let symbol = match &events
        .first()
        .unwrap_or_else(|| panic!("scenario {input_path} is empty"))
        .kind
    {
        ReplayEventKind::NewOrder { order } => order.symbol.clone(),
        ReplayEventKind::Cancel { symbol, .. } => symbol.clone(),
    };

    let mut driver = ReplayDriver::new(MatchingEngine::new(symbol));
    let result = driver.replay_events(events);
    (driver, result)
}

fn assert_replay_matches_golden(input_path: &str, expected_path: &str) -> ReplayResult {
    let expected_path = workspace_path(expected_path);
    let (_, result) = replay_scenario(input_path);
    let result = result.unwrap_or_else(|error| panic!("failed to replay {input_path}: {error}"));
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
        workspace_path(input_path).display(),
        expected_path.display()
    );

    result
}

fn assert_replay_error_matches_golden(
    input_path: &str,
    expected_path: &str,
) -> (ReplayDriver, ReplayError) {
    let expected_path = workspace_path(expected_path);
    let expected = fs::read_to_string(&expected_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", expected_path.display()));
    let (driver, result) = replay_scenario(input_path);
    let error = result.unwrap_err();
    let actual = format!("{error}\n");

    assert_eq!(
        actual,
        expected,
        "replay error for {} did not match {}",
        workspace_path(input_path).display(),
        expected_path.display()
    );

    (driver, error)
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

#[test]
fn empty_book_market_order_matches_golden() {
    let result = assert_replay_matches_golden(
        "data/scenarios/empty_book_market_order.jsonl",
        "data/expected/empty_book_market_order.out.jsonl",
    );

    assert_eq!(result.summary.rejected, 1);
    assert!(result.final_book.bids.is_empty());
    assert!(result.final_book.asks.is_empty());
}

#[test]
fn multi_level_fill_matches_golden() {
    let result = assert_replay_matches_golden(
        "data/scenarios/multi_level_fill.jsonl",
        "data/expected/multi_level_fill.out.jsonl",
    );

    assert_eq!(result.summary.trades, 2);
    assert!(result.final_book.asks.is_empty());
}

#[test]
fn fifo_priority_matches_golden() {
    let result = assert_replay_matches_golden(
        "data/scenarios/fifo_priority.jsonl",
        "data/expected/fifo_priority.out.jsonl",
    );

    assert_eq!(result.summary.trades, 2);
    assert!(result.final_book.asks.is_empty());
}

#[test]
fn cancel_after_partial_fill_matches_golden() {
    let result = assert_replay_matches_golden(
        "data/scenarios/cancel_after_partial_fill.jsonl",
        "data/expected/cancel_after_partial_fill.out.jsonl",
    );

    assert_eq!(result.summary.cancelled, 1);
    assert!(result.final_book.bids.is_empty());
    assert!(result.final_book.asks.is_empty());
}

#[test]
fn duplicate_sequence_matches_fatal_error_golden() {
    let (driver, error) = assert_replay_error_matches_golden(
        "data/scenarios/duplicate_seq_rejected.jsonl",
        "data/expected/duplicate_seq_rejected.err.txt",
    );

    assert!(matches!(error, ReplayError::DuplicateSequence { seq: 1 }));
    assert_eq!(driver.last_seq(), None);
    assert!(driver.engine().book().snapshot(usize::MAX).bids.is_empty());
    assert!(driver.engine().book().snapshot(usize::MAX).asks.is_empty());
}

#[test]
fn out_of_order_sequence_matches_fatal_error_golden() {
    let (driver, error) = assert_replay_error_matches_golden(
        "data/scenarios/out_of_order_seq_rejected.jsonl",
        "data/expected/out_of_order_seq_rejected.err.txt",
    );

    assert!(matches!(
        error,
        ReplayError::OutOfOrderSequence {
            previous: 2,
            seq: 1
        }
    ));
    assert_eq!(driver.last_seq(), None);
    assert!(driver.engine().book().snapshot(usize::MAX).bids.is_empty());
    assert!(driver.engine().book().snapshot(usize::MAX).asks.is_empty());
}

#[test]
fn crossed_book_prevention_matches_golden() {
    let result = assert_replay_matches_golden(
        "data/scenarios/crossed_book_prevention.jsonl",
        "data/expected/crossed_book_prevention.out.jsonl",
    );

    assert_eq!(result.summary.trades, 2);
    assert_eq!(result.summary.final_resting_orders, 0);
    assert!(result.final_book.bids.is_empty());
    assert!(result.final_book.asks.is_empty());
}
