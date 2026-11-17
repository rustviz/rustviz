//! Where accepted records go.
//!
//! * `stdout`: one Cloud Logging structured-log line per record. Cloud Run
//!   ships stdout to Cloud Logging; a log sink filtered on the
//!   `rv_stream` label routes event lines to BigQuery dataset `rv_research`
//!   and program lines to `rv_programs` (see `infra/`). The whole record is
//!   carried as one JSON *string* (`rv_json`) so the sink's auto-generated
//!   BigQuery schema is a single STRING column that can never conflict
//!   between event types (and never meets an array of arrays, which
//!   BigQuery can't store); the SQL views parse it with `JSON_VALUE`.
//! * `file`: the canonical records as JSONL, events and programs in two
//!   files (local dev, tests, and the dashboard's file mode).
//!
//! Records contain only what `schema::validate` built; no request metadata
//! reaches a sink.

use serde_json::{json, Map, Value};
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Cloud Logging label that log sinks filter on.
pub const STREAM_LABEL: &str = "rv_stream";
pub const EVENTS_STREAM: &str = "rv_events";
pub const PROGRAMS_STREAM: &str = "rv_programs";

pub trait Sink: Send + Sync {
    fn write(
        &self,
        events: &[Map<String, Value>],
        programs: &[Map<String, Value>],
    ) -> io::Result<()>;
}

/// Cloud Logging structured line for one record.
pub fn cloud_logging_line(stream: &str, record: &Map<String, Value>) -> String {
    let kind = if stream == PROGRAMS_STREAM {
        "rv_program"
    } else {
        "rv_event"
    };
    let line = json!({
        "severity": "INFO",
        "message": kind,
        "logging.googleapis.com/labels": { STREAM_LABEL: stream },
        "event": record.get("event").cloned().unwrap_or(Value::Null),
        "rv_json": Value::Object(record.clone()).to_string(),
    });
    line.to_string()
}

pub struct StdoutSink;

impl Sink for StdoutSink {
    fn write(
        &self,
        events: &[Map<String, Value>],
        programs: &[Map<String, Value>],
    ) -> io::Result<()> {
        // Build the whole batch first, then one locked write, so lines from
        // concurrent requests never interleave.
        let mut buf = String::new();
        for r in events {
            buf.push_str(&cloud_logging_line(EVENTS_STREAM, r));
            buf.push('\n');
        }
        for r in programs {
            buf.push_str(&cloud_logging_line(PROGRAMS_STREAM, r));
            buf.push('\n');
        }
        let mut out = io::stdout().lock();
        out.write_all(buf.as_bytes())?;
        out.flush()
    }
}

pub struct FileSink {
    files: Mutex<(File, File)>,
    pub events_path: PathBuf,
    pub programs_path: PathBuf,
}

impl FileSink {
    pub fn open(events_path: &Path, programs_path: &Path) -> io::Result<Self> {
        let open = |p: &Path| OpenOptions::new().create(true).append(true).open(p);
        Ok(FileSink {
            files: Mutex::new((open(events_path)?, open(programs_path)?)),
            events_path: events_path.to_path_buf(),
            programs_path: programs_path.to_path_buf(),
        })
    }
}

fn jsonl(records: &[Map<String, Value>]) -> String {
    let mut buf = String::new();
    for r in records {
        buf.push_str(&Value::Object(r.clone()).to_string());
        buf.push('\n');
    }
    buf
}

impl Sink for FileSink {
    fn write(
        &self,
        events: &[Map<String, Value>],
        programs: &[Map<String, Value>],
    ) -> io::Result<()> {
        let mut files = self.files.lock().unwrap_or_else(|e| e.into_inner());
        if !events.is_empty() {
            files.0.write_all(jsonl(events).as_bytes())?;
            files.0.flush()?;
        }
        if !programs.is_empty() {
            files.1.write_all(jsonl(programs).as_bytes())?;
            files.1.flush()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_logging_line_shape() {
        let mut r = Map::new();
        r.insert("event".into(), json!("page_view"));
        r.insert("props".into(), json!({"samples": [[0, 1.5, 2.0]]}));
        let line: Value = serde_json::from_str(&cloud_logging_line(EVENTS_STREAM, &r)).unwrap();
        assert_eq!(line["severity"], "INFO");
        assert_eq!(
            line["logging.googleapis.com/labels"]["rv_stream"],
            "rv_events"
        );
        assert_eq!(line["event"], "page_view");
        let inner: Value = serde_json::from_str(line["rv_json"].as_str().unwrap()).unwrap();
        assert_eq!(inner, Value::Object(r));
    }
}
