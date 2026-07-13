use std::io::{self, BufRead};

use thiserror::Error;

use super::ReplayEvent;

#[derive(Debug, Error)]
pub enum ReplayParseError {
    #[error("failed to read replay line {line}: {source}")]
    ReadLine {
        line: usize,
        #[source]
        source: io::Error,
    },
    #[error("invalid replay JSON on line {line}: {source}")]
    MalformedJson {
        line: usize,
        #[source]
        source: serde_json::Error,
    },
}

pub fn parse_jsonl<R: BufRead>(reader: R) -> Result<Vec<ReplayEvent>, ReplayParseError> {
    let mut events = Vec::new();

    for (line_index, line) in reader.lines().enumerate() {
        let line_number = line_index + 1;
        let line = line.map_err(|source| ReplayParseError::ReadLine {
            line: line_number,
            source,
        })?;

        if line.trim().is_empty() {
            continue;
        }

        let event =
            serde_json::from_str(&line).map_err(|source| ReplayParseError::MalformedJson {
                line: line_number,
                source,
            })?;
        events.push(event);
    }

    Ok(events)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::{
        replay::ReplayEventKind,
        types::{OrderType, PriceTicks, Qty, Side, Symbol},
    };

    #[test]
    fn empty_and_whitespace_only_input_has_no_events() {
        assert!(parse_jsonl(Cursor::new("")).unwrap().is_empty());
        assert!(parse_jsonl(Cursor::new("\n  \n\t\n")).unwrap().is_empty());
    }

    #[test]
    fn parses_records_separated_by_empty_lines() {
        let input = concat!(
            "\n",
            r#"{"seq":1,"ts_ns":10,"kind":"new_order","order":{"order_id":7,"symbol":"AAPL","side":"Buy","order_type":"Limit","price":100,"qty":5,"timestamp_ns":9,"strategy_id":null}}"#,
            "\n  \n",
            r#"{"seq":2,"ts_ns":20,"kind":"cancel","order_id":7,"symbol":"AAPL"}"#,
            "\n",
        );

        let events = parse_jsonl(Cursor::new(input)).unwrap();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].seq, 1);
        assert_eq!(events[0].ts_ns, 10);
        assert!(matches!(
            &events[0].kind,
            ReplayEventKind::NewOrder { order }
                if order.order_type == OrderType::Limit
                    && order.price == Some(PriceTicks(100))
                    && order.qty == Qty(5)
                    && order.side == Side::Buy
                    && order.symbol == Symbol("AAPL".to_owned())
        ));
        assert_eq!(events[1].seq, 2);
        assert!(matches!(
            &events[1].kind,
            ReplayEventKind::Cancel { order_id: 7, symbol }
                if symbol == &Symbol("AAPL".to_owned())
        ));
    }

    #[test]
    fn malformed_json_reports_physical_line_number() {
        let input = "\n  \n{not-json}\n";

        let error = parse_jsonl(Cursor::new(input)).unwrap_err();

        assert!(matches!(
            error,
            ReplayParseError::MalformedJson { line: 3, .. }
        ));
        assert!(error.to_string().contains("line 3"));
    }
}
