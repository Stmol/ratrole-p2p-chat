//! Correlated, secret-safe JSONL diagnostics for one Rathole process.
//!
//! The logger writes one flushed record per event. A process-local sequence,
//! instance ID, peer ID, connection ID, stream ID, and message ID let paired
//! clients correlate a delivery attempt without recording message bodies or
//! device secrets. Logging is best-effort after initialization: a failed write
//! is not allowed to take down the chat runtime.

use std::{
    collections::BTreeMap,
    env,
    fmt::{self, Display, Write as FmtWrite},
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    sync::{Arc, Mutex, OnceLock},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use rand::RngExt;
use serde::Serialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{domain::identity::PeerId, protocol::MessageId};

const LOG_SCHEMA_VERSION: u8 = 2;
const LOG_FILE_ENV: &str = "RATHOLE_LOG_FILE";

static GLOBAL_LOGGER: OnceLock<Arc<Logger>> = OnceLock::new();

#[derive(Default, Serialize)]
pub(crate) struct LogFields {
    /// Remote or related peer identifier, when the event has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) peer_id: Option<String>,
    /// Correlation identifier of the related protocol message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) message_id: Option<String>,
    /// Iroh connection stable ID associated with the event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) connection_id: Option<usize>,
    /// QUIC stream ID associated with the event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) stream_id: Option<u64>,
    /// Message body size in bytes; the body itself is never logged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) body_bytes: Option<usize>,
    /// Sender-provided message timestamp in Unix milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) sent_at_unix_ms: Option<i64>,
    /// Receiver-provided acceptance timestamp in Unix milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) received_at_unix_ms: Option<i64>,
    /// Elapsed duration measured by the emitting process.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) duration_ms: Option<u64>,
    /// Number of contacts involved in a startup or allowlist event.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) contact_count: Option<usize>,
    /// Direction label such as `inbound` or `outbound`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) direction: Option<String>,
    /// Event-specific status label.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) status: Option<String>,
    /// Machine-readable or human-readable reason for a warning or transition.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reason: Option<String>,
    /// Stringified error information, when an operation failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
    /// Extensible event-specific fields with stable string values.
    #[serde(flatten)]
    pub(crate) details: BTreeMap<String, String>,
}

impl LogFields {
    /// Adds a domain peer identifier to the event.
    pub(crate) fn peer(mut self, peer_id: &PeerId) -> Self {
        self.peer_id = Some(peer_id.as_str().to_owned());
        self
    }

    /// Adds an already formatted peer identifier to the event.
    pub(crate) fn peer_str(mut self, peer_id: impl Into<String>) -> Self {
        self.peer_id = Some(peer_id.into());
        self
    }

    /// Adds a hexadecimal message correlation identifier to the event.
    pub(crate) fn message(mut self, message_id: &MessageId) -> Self {
        self.message_id = Some(message_id_string(message_id));
        self
    }

    /// Adds an Iroh connection stable ID to the event.
    pub(crate) fn connection(mut self, connection_id: usize) -> Self {
        self.connection_id = Some(connection_id);
        self
    }

    /// Adds a transport stream ID to the event.
    pub(crate) fn stream(mut self, stream_id: u64) -> Self {
        self.stream_id = Some(stream_id);
        self
    }

    /// Records a body length without recording the body contents.
    pub(crate) fn body_bytes(mut self, body_bytes: usize) -> Self {
        self.body_bytes = Some(body_bytes);
        self
    }

    /// Records a sender-provided Unix-millisecond timestamp.
    pub(crate) fn sent_at(mut self, sent_at_unix_ms: i64) -> Self {
        self.sent_at_unix_ms = Some(sent_at_unix_ms);
        self
    }

    /// Records an elapsed operation duration in milliseconds.
    pub(crate) fn duration(mut self, duration_ms: u64) -> Self {
        self.duration_ms = Some(duration_ms);
        self
    }

    /// Records the number of contacts represented by the event.
    pub(crate) fn contacts(mut self, contact_count: usize) -> Self {
        self.contact_count = Some(contact_count);
        self
    }

    /// Adds a direction label to the event.
    pub(crate) fn direction(mut self, direction: impl Into<String>) -> Self {
        self.direction = Some(direction.into());
        self
    }

    /// Adds a status label to the event.
    pub(crate) fn status(mut self, status: impl Into<String>) -> Self {
        self.status = Some(status.into());
        self
    }

    /// Adds a reason label to the event.
    pub(crate) fn reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// Adds a displayable error without retaining the error object itself.
    pub(crate) fn error(mut self, error: impl Display) -> Self {
        self.error = Some(error.to_string());
        self
    }

    /// Adds an event-specific string field to the flattened detail map.
    pub(crate) fn detail(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.details.insert(key.into(), value.into());
        self
    }
}

/// Process-local writer that serializes correlated diagnostic events to JSONL.
pub(crate) struct Logger {
    /// Random identifier shared by all records from this process instance.
    instance_id: String,
    /// Canonical local peer identity included for cross-client correlation.
    local_peer_id: String,
    /// Monotonic origin used to compute process-relative elapsed milliseconds.
    started_at: Instant,
    /// Atomic record counter preserving unique event sequence values.
    next_sequence: AtomicU64,
    /// Mutex protecting line ordering and flushes across concurrent callers.
    file: Mutex<File>,
    /// Effective log path selected during initialization.
    path: PathBuf,
}

/// Serialized record shape written as one JSON line.
#[derive(Serialize)]
struct LogRecord {
    schema_version: u8,
    app_version: &'static str,
    ts_unix_ms: i64,
    ts_utc: String,
    monotonic_ms: u64,
    instance_id: String,
    seq: u64,
    event_id: String,
    level: &'static str,
    component: &'static str,
    event: &'static str,
    local_peer_id: String,
    #[serde(flatten)]
    fields: LogFields,
}

impl Logger {
    /// Creates and globally installs the process logger.
    ///
    /// `RATHOLE_LOG_FILE` overrides the default per-instance file below the
    /// application data directory. Initialization fails if a global logger was
    /// already installed or if the target file cannot be created exclusively.
    pub(crate) fn init(data_dir: &Path, local_peer_id: &PeerId) -> io::Result<Arc<Self>> {
        let instance_id = random_token();
        let path = env::var_os(LOG_FILE_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                data_dir
                    .join("logs")
                    .join(format!("rathole-{instance_id}.jsonl"))
            });
        let logger = Arc::new(Self::open(path, local_peer_id, instance_id)?);
        GLOBAL_LOGGER.set(logger.clone()).map_err(|_| {
            io::Error::new(io::ErrorKind::AlreadyExists, "logger already initialized")
        })?;
        logger.event(
            "info",
            "logging",
            "logger_initialized",
            LogFields::default()
                .detail("log_file", logger.path.display().to_string())
                .detail("app_version", env!("CARGO_PKG_VERSION")),
        );
        Ok(logger)
    }

    /// Creates the append-only file and initializes process correlation state.
    fn open(path: PathBuf, local_peer_id: &PeerId, instance_id: String) -> io::Result<Self> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)?;
        Ok(Self {
            instance_id,
            local_peer_id: local_peer_id.as_str().to_owned(),
            started_at: Instant::now(),
            next_sequence: AtomicU64::new(0),
            file: Mutex::new(file),
            path,
        })
    }

    /// Returns the effective path of the JSONL log file.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// Serializes and flushes one best-effort diagnostic record.
    ///
    /// A poisoned lock, serialization failure, or file write error is ignored
    /// after the event has been constructed; diagnostics must not become a new
    /// failure mode for the application or transport.
    fn event(
        &self,
        level: &'static str,
        component: &'static str,
        event: &'static str,
        fields: LogFields,
    ) {
        let Ok(mut file) = self.file.lock() else {
            return;
        };
        let seq = self.next_sequence.fetch_add(1, Ordering::Relaxed) + 1;
        let ts_unix_ms = unix_ms_now();
        let ts_utc = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_else(|_| format!("unix:{ts_unix_ms}"));
        let monotonic_ms = self
            .started_at
            .elapsed()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX);
        let record = LogRecord {
            schema_version: LOG_SCHEMA_VERSION,
            app_version: env!("CARGO_PKG_VERSION"),
            ts_unix_ms,
            ts_utc,
            monotonic_ms,
            instance_id: self.instance_id.clone(),
            seq,
            event_id: format!("{}-{seq:08}", self.instance_id),
            level,
            component,
            event,
            local_peer_id: self.local_peer_id.clone(),
            fields,
        };
        let Ok(mut line) = serde_json::to_vec(&record) else {
            return;
        };
        line.push(b'\n');
        let _ = file.write_all(&line);
        let _ = file.flush();
    }
}

/// Emits an informational event when the global logger has been initialized.
pub(crate) fn log_event(component: &'static str, event: &'static str, fields: LogFields) {
    if let Some(logger) = GLOBAL_LOGGER.get() {
        logger.event("info", component, event, fields);
    }
}

/// Emits a warning event when the global logger has been initialized.
pub(crate) fn log_warn(component: &'static str, event: &'static str, fields: LogFields) {
    if let Some(logger) = GLOBAL_LOGGER.get() {
        logger.event("warn", component, event, fields);
    }
}

/// Generates the process instance token used in file names and event IDs.
fn random_token() -> String {
    let bytes = rand::rng().random::<[u8; 16]>();
    let mut token = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(token, "{byte:02x}");
    }
    token
}

/// Formats a binary message ID as a stable lowercase hexadecimal string.
fn message_id_string(message_id: &MessageId) -> String {
    let mut value = String::with_capacity(message_id.as_bytes().len() * 2);
    for byte in message_id.as_bytes() {
        let _ = write!(value, "{byte:02x}");
    }
    value
}

/// Returns the current wall-clock time as Unix milliseconds, falling back to
/// zero when the system clock predates the Unix epoch.
fn unix_ms_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

impl fmt::Debug for Logger {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Logger")
            .field("instance_id", &self.instance_id)
            .field("local_peer_id", &self.local_peer_id)
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn writes_flushed_jsonl_records_with_correlation_metadata() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("chat.jsonl");
        let peer_id = PeerId::from_canonical("local-peer".to_owned());
        let logger = Logger::open(path.clone(), &peer_id, "instance-test".to_owned()).unwrap();
        let message_id = MessageId::new([0xab; 16]);

        logger.event(
            "info",
            "chat",
            "message_queued",
            LogFields::default()
                .peer_str("remote-peer")
                .message(&message_id)
                .body_bytes(12),
        );
        logger.event(
            "warn",
            "chat",
            "message_delivery_settled",
            LogFields::default()
                .peer_str("remote-peer")
                .message(&message_id)
                .status("timed_out")
                .reason("deadline"),
        );

        let contents = fs::read_to_string(path).unwrap();
        let records = contents
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0]["schema_version"], 2);
        assert_eq!(records[0]["app_version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(records[0]["instance_id"], "instance-test");
        assert_eq!(records[0]["local_peer_id"], "local-peer");
        assert_eq!(records[0]["message_id"], "ab".repeat(16));
        assert_eq!(records[0]["body_bytes"], 12);
        assert!(records[0].get("body").is_none());
        assert_eq!(records[0]["seq"], 1);
        assert_eq!(records[1]["seq"], 2);
        assert!(records[1]["monotonic_ms"].as_u64().is_some());
        assert!(records[1]["ts_utc"].as_str().unwrap().contains('T'));
    }

    #[test]
    fn creates_the_parent_directory_for_a_log_file() {
        let directory = tempfile::tempdir().unwrap();
        let peer_id = PeerId::from_canonical("local-peer".to_owned());
        let path = directory.path().join("logs").join("rathole-test.jsonl");
        let logger = Logger::open(path.clone(), &peer_id, "test".to_owned()).unwrap();
        assert_eq!(logger.path(), path);
        assert!(logger.path().parent().unwrap().is_dir());
    }

    #[test]
    fn sequence_matches_line_order_when_tasks_log_concurrently() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("chat.jsonl");
        let peer_id = PeerId::from_canonical("local-peer".to_owned());
        let logger =
            Arc::new(Logger::open(path.clone(), &peer_id, "instance-test".to_owned()).unwrap());
        let mut threads = Vec::new();
        for _ in 0..4 {
            let logger = logger.clone();
            threads.push(std::thread::spawn(move || {
                for _ in 0..10 {
                    logger.event("info", "test", "concurrent_event", LogFields::default());
                }
            }));
        }
        for thread in threads {
            thread.join().unwrap();
        }

        let records = fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
            .collect::<Vec<_>>();
        let sequence = records
            .iter()
            .map(|record| record["seq"].as_u64().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(sequence, (1..=40).collect::<Vec<_>>());
    }
}
