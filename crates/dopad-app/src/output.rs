use std::{
    collections::VecDeque,
    fmt,
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

use tracing::{Event, Subscriber, field::Visit};
use tracing_subscriber::{Layer, layer::Context};

const MAX_ENTRIES: usize = 4_000;
const MAX_BUFFER_BYTES: usize = 1_048_576;
const MAX_ENTRY_BYTES: usize = 4_096;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum OutputChannel {
    General,
    Lsp,
}

impl OutputChannel {
    pub(crate) const ALL: [Self; 2] = [Self::General, Self::Lsp];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Lsp => "LSP",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct OutputEntry {
    pub sequence: u64,
    pub timestamp: String,
    pub level: String,
    pub target: String,
    pub message: String,
}

impl OutputEntry {
    pub(crate) fn line(&self) -> String {
        format!(
            "{} {:<5} {}: {}",
            self.timestamp, self.level, self.target, self.message
        )
    }

    fn byte_len(&self) -> usize {
        self.timestamp.len() + self.level.len() + self.target.len() + self.message.len() + 8
    }
}

#[derive(Default)]
struct ChannelBuffer {
    entries: VecDeque<OutputEntry>,
    bytes: usize,
    revision: u64,
    next_sequence: u64,
}

impl ChannelBuffer {
    fn push(&mut self, mut entry: OutputEntry) {
        entry.sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);
        self.bytes = self.bytes.saturating_add(entry.byte_len());
        self.entries.push_back(entry);
        while self.entries.len() > MAX_ENTRIES || self.bytes > MAX_BUFFER_BYTES {
            let Some(removed) = self.entries.pop_front() else {
                break;
            };
            self.bytes = self.bytes.saturating_sub(removed.byte_len());
        }
        self.revision = self.revision.wrapping_add(1);
    }

    fn clear(&mut self) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        self.entries.clear();
        self.bytes = 0;
        self.revision = self.revision.wrapping_add(1);
        true
    }
}

#[derive(Default)]
struct OutputBuffers {
    general: ChannelBuffer,
    lsp: ChannelBuffer,
}

impl OutputBuffers {
    fn get(&self, channel: OutputChannel) -> &ChannelBuffer {
        match channel {
            OutputChannel::General => &self.general,
            OutputChannel::Lsp => &self.lsp,
        }
    }

    fn get_mut(&mut self, channel: OutputChannel) -> &mut ChannelBuffer {
        match channel {
            OutputChannel::General => &mut self.general,
            OutputChannel::Lsp => &mut self.lsp,
        }
    }
}

fn buffers() -> &'static Mutex<OutputBuffers> {
    static BUFFERS: OnceLock<Mutex<OutputBuffers>> = OnceLock::new();
    BUFFERS.get_or_init(|| Mutex::new(OutputBuffers::default()))
}

pub(crate) fn revision(channel: OutputChannel) -> u64 {
    buffers()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(channel)
        .revision
}

pub(crate) fn snapshot(channel: OutputChannel) -> (u64, Vec<OutputEntry>) {
    let buffers = buffers()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let buffer = buffers.get(channel);
    (buffer.revision, buffer.entries.iter().cloned().collect())
}

pub(crate) fn clear(channel: OutputChannel) -> bool {
    buffers()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get_mut(channel)
        .clear()
}

pub(crate) struct OutputLogLayer;

impl<S> Layer<S> for OutputLogLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _: Context<'_, S>) {
        let metadata = event.metadata();
        let channel = if metadata.target().contains("lsp") {
            OutputChannel::Lsp
        } else {
            OutputChannel::General
        };
        let mut visitor = EventVisitor::default();
        event.record(&mut visitor);
        let mut message = visitor.message.unwrap_or_default();
        if !visitor.fields.is_empty() {
            if !message.is_empty() {
                message.push_str(" · ");
            }
            message.push_str(&visitor.fields.join(" "));
        }
        let message = truncate_utf8(message, MAX_ENTRY_BYTES);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .to_string();
        let entry = OutputEntry {
            sequence: 0,
            timestamp,
            level: metadata.level().to_string(),
            target: truncate_utf8(metadata.target().to_owned(), 192),
            message,
        };
        buffers()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get_mut(channel)
            .push(entry);
    }
}

#[derive(Default)]
struct EventVisitor {
    message: Option<String>,
    fields: Vec<String>,
}

impl Visit for EventVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        let value = format!("{value:?}");
        if field.name() == "message" {
            self.message = Some(value.trim_matches('"').to_owned());
        } else {
            self.fields.push(format!("{}={value}", field.name()));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_owned());
        } else {
            self.fields.push(format!("{}={value}", field.name()));
        }
    }
}

fn truncate_utf8(mut value: String, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value;
    }
    let mut boundary = max_bytes.saturating_sub(3);
    while !value.is_char_boundary(boundary) {
        boundary = boundary.saturating_sub(1);
    }
    value.truncate(boundary);
    value.push_str("...");
    value
}

#[cfg(test)]
mod tests {
    use super::{
        ChannelBuffer, MAX_BUFFER_BYTES, MAX_ENTRY_BYTES, OutputChannel, OutputEntry, truncate_utf8,
    };

    fn entry(message: String) -> OutputEntry {
        OutputEntry {
            sequence: 0,
            timestamp: "0".into(),
            level: "INFO".into(),
            target: "test".into(),
            message,
        }
    }

    #[test]
    fn channel_labels_only_expose_implemented_sources() {
        assert_eq!(
            OutputChannel::ALL.map(OutputChannel::label),
            ["General", "LSP"]
        );
    }

    #[test]
    fn buffer_discards_old_entries_when_byte_limit_is_reached() {
        let mut buffer = ChannelBuffer::default();
        for _ in 0..MAX_BUFFER_BYTES / MAX_ENTRY_BYTES + 4 {
            buffer.push(entry("x".repeat(MAX_ENTRY_BYTES)));
        }
        assert!(buffer.bytes <= MAX_BUFFER_BYTES);
        assert!(buffer.entries.len() < MAX_BUFFER_BYTES / MAX_ENTRY_BYTES + 4);
    }

    #[test]
    fn truncation_preserves_utf8_boundaries() {
        let truncated = truncate_utf8("å".repeat(32), 12);
        assert!(truncated.is_char_boundary(truncated.len()));
        assert!(truncated.len() <= 12);
        assert!(truncated.ends_with("..."));
    }
}

