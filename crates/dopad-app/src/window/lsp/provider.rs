use std::{
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI32, Ordering},
    },
    time::Duration,
};

use anyhow::Context as _;
use gpui_kit::base::input::RopeExt as _;
use gpui_kit::component::input::{CompletionProvider, Rope};
use gpui_kit::{App, BackgroundExecutor, Task, Window};
use lsp_types::{
    CompletionContext, CompletionItem, CompletionResponse, InlineCompletionContext,
    InlineCompletionItem, InlineCompletionResponse, Position,
};
use serde_json::json;

use super::client::LspClient;

const COMPLETION_DEBOUNCE: Duration = Duration::from_millis(55);
const COMPLETION_TIMEOUT: Duration = Duration::from_secs(3);
const INLINE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub(super) struct LspDocument {
    client: LspClient,
    uri: String,
    language_id: String,
    version: Arc<AtomicI32>,
    ready: Arc<AtomicBool>,
    closed: Arc<AtomicBool>,
    open_gate: Arc<Mutex<(bool, Vec<serde_json::Value>)>>,
    ready_signal: async_channel::Receiver<()>,
    ready_sender: async_channel::Sender<()>,
}

impl LspDocument {
    pub(super) fn new(client: LspClient, uri: String, language_id: String) -> Self {
        let (ready_sender, ready_signal) = async_channel::bounded(1);
        Self {
            client,
            uri,
            language_id,
            version: Arc::new(AtomicI32::new(1)),
            ready: Arc::new(AtomicBool::new(false)),
            closed: Arc::new(AtomicBool::new(false)),
            open_gate: Arc::new(Mutex::new((false, Vec::new()))),
            ready_signal,
            ready_sender,
        }
    }

    pub(super) fn provider(&self) -> Rc<dyn CompletionProvider> {
        Rc::new(LspCompletionProvider {
            document: self.clone(),
        })
    }

    pub(super) fn did_open(&self, text: String) -> anyhow::Result<()> {
        let mut gate = self
            .open_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("LSP document open lock poisoned"))?;
        anyhow::ensure!(
            !self.closed.load(Ordering::Acquire),
            "LSP document is closed"
        );
        self.client.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": self.uri,
                    "languageId": self.language_id,
                    "version": 1,
                    "text": text
                }
            }),
        )?;
        for change in gate.1.drain(..) {
            self.client.notify("textDocument/didChange", change)?;
        }
        gate.0 = true;
        self.ready.store(true, Ordering::Release);
        self.ready_sender.close();
        Ok(())
    }

    pub(super) fn did_change(
        &self,
        range: Option<lsp_types::Range>,
        range_length: Option<u32>,
        text: String,
    ) -> anyhow::Result<()> {
        if self.closed.load(Ordering::Acquire) {
            return Ok(());
        }
        let version = self
            .version
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                Some(current.saturating_add(1))
            })
            .map_or(i32::MAX, |previous| previous.saturating_add(1));
        let params = json!({
            "textDocument": { "uri": self.uri, "version": version },
            "contentChanges": [{ "range": range, "rangeLength": range_length, "text": text }]
        });
        let mut gate = self
            .open_gate
            .lock()
            .map_err(|_| anyhow::anyhow!("LSP document change lock poisoned"))?;
        if self.closed.load(Ordering::Acquire) {
            return Ok(());
        }
        if gate.0 {
            self.client.notify("textDocument/didChange", params)
        } else {
            gate.1.push(params);
            Ok(())
        }
    }

    pub(super) fn did_close(&self) {
        if let Ok(mut gate) = self.open_gate.lock() {
            self.closed.store(true, Ordering::Release);
            self.ready_sender.close();
            if gate.0 {
                let _ = self.client.notify(
                    "textDocument/didClose",
                    json!({ "textDocument": { "uri": self.uri } }),
                );
            }
            gate.1.clear();
        }
    }

    async fn wait_until_open(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.closed.load(Ordering::Acquire),
            "LSP document is closed"
        );
        if self.ready.load(Ordering::Acquire) {
            return Ok(());
        }
        let _ = self.ready_signal.recv().await;
        anyhow::ensure!(
            self.ready.load(Ordering::Acquire),
            "LSP document failed to open"
        );
        Ok(())
    }

    async fn request_completions(
        &self,
        text: &Rope,
        offset: usize,
        context: Option<CompletionContext>,
        executor: &BackgroundExecutor,
        timeout: Duration,
    ) -> anyhow::Result<CompletionResponse> {
        self.wait_until_open().await?;
        let position = offset_to_position(text, offset);
        let result = self
            .client
            .request(
                "textDocument/completion",
                json!({
                    "textDocument": { "uri": self.uri },
                    "position": position,
                    "context": context
                }),
                executor,
                timeout,
            )
            .await?;
        if result.is_null() {
            return Ok(CompletionResponse::Array(Vec::new()));
        }
        let mut response: CompletionResponse = serde_json::from_value(result)
            .context("invalid completion response from language server")?;
        // gpui-kit cannot safely apply snippets or additional edits.
        match &mut response {
            CompletionResponse::Array(items) => items.retain(is_safe_completion),
            CompletionResponse::List(list) => list.items.retain(is_safe_completion),
        }
        Ok(response)
    }

    async fn resolve_completion(
        &self,
        item: &CompletionItem,
        executor: &BackgroundExecutor,
    ) -> anyhow::Result<CompletionItem> {
        let result = self
            .client
            .request(
                "completionItem/resolve",
                serde_json::to_value(item)?,
                executor,
                COMPLETION_TIMEOUT,
            )
            .await?;
        serde_json::from_value(result).context("invalid completionItem/resolve response")
    }
}

struct LspCompletionProvider {
    document: LspDocument,
}

impl CompletionProvider for LspCompletionProvider {
    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        trigger: CompletionContext,
        _: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<CompletionResponse>> {
        let document = self.document.clone();
        let snapshot = text.clone();
        let executor = cx.background_executor().clone();
        executor.clone().spawn(async move {
            executor.timer(COMPLETION_DEBOUNCE).await;
            let trigger_character = trigger
                .trigger_character
                .filter(|value| value.chars().count() == 1);
            let context = CompletionContext {
                trigger_kind: if trigger_character.is_some() {
                    lsp_types::CompletionTriggerKind::TRIGGER_CHARACTER
                } else {
                    lsp_types::CompletionTriggerKind::INVOKED
                },
                trigger_character,
            };
            document
                .request_completions(
                    &snapshot,
                    offset,
                    Some(context),
                    &executor,
                    COMPLETION_TIMEOUT,
                )
                .await
        })
    }

    fn inline_completion(
        &self,
        text: &Rope,
        offset: usize,
        _: InlineCompletionContext,
        _: &mut Window,
        cx: &mut App,
    ) -> Task<anyhow::Result<InlineCompletionResponse>> {
        let document = self.document.clone();
        let snapshot = text.clone();
        let executor = cx.background_executor().clone();
        executor.clone().spawn(async move {
            document.wait_until_open().await?;
            let response = document
                .request_completions(&snapshot, offset, None, &executor, INLINE_TIMEOUT)
                .await?;
            let items = match response {
                CompletionResponse::Array(items) => items,
                CompletionResponse::List(list) => list.items,
            };
            let prefix = current_word(&snapshot, offset);
            let Some(mut item) = items
                .into_iter()
                .find(|item| safe_inline_suffix(item, &prefix).is_some())
            else {
                return Ok(InlineCompletionResponse::Array(Vec::new()));
            };
            if item.data.is_some()
                && let Ok(resolved) = document.resolve_completion(&item, &executor).await
                && is_safe_completion(&resolved)
            {
                item = resolved;
            }
            let Some(suffix) = safe_inline_suffix(&item, &prefix) else {
                return Ok(InlineCompletionResponse::Array(Vec::new()));
            };
            Ok(InlineCompletionResponse::Array(vec![
                InlineCompletionItem {
                    insert_text: suffix,
                    filter_text: None,
                    range: None,
                    command: None,
                    insert_text_format: None,
                },
            ]))
        })
    }

    fn inline_completion_debounce(&self) -> Duration {
        Duration::from_millis(350)
    }

    fn is_completion_trigger(&self, _: usize, new_text: &str, _: &mut App) -> bool {
        new_text.chars().any(|character| {
            character.is_alphanumeric() || character == '_' || matches!(character, '.' | ':' | '$')
        })
    }
}

pub(super) fn offset_to_position(text: &Rope, byte_offset: usize) -> Position {
    let point = text.offset_to_point(byte_offset.min(text.len()));
    let line = text.slice_line(point.row);
    let utf16_column = line.byte_to_utf16_idx(point.column.min(line.len()));
    Position::new(
        u32::try_from(point.row).unwrap_or(u32::MAX),
        u32::try_from(utf16_column).unwrap_or(u32::MAX),
    )
}

fn current_word(text: &Rope, byte_offset: usize) -> String {
    let cursor = byte_offset.min(text.len());
    let point = text.offset_to_point(cursor);
    let start = text.line_start_offset(point.row);
    let prefix = text.slice(start..cursor).to_string();
    let word_start = prefix
        .char_indices()
        .rev()
        .take_while(|(_, character)| character.is_alphanumeric() || *character == '_')
        .last()
        .map_or(prefix.len(), |(index, _)| index);
    prefix[word_start..].to_owned()
}

fn safe_inline_suffix(item: &CompletionItem, prefix: &str) -> Option<String> {
    if !is_safe_completion(item) {
        return None;
    }
    let text = item.insert_text.as_deref().unwrap_or(&item.label);
    text.strip_prefix(prefix)
        .filter(|suffix| !suffix.is_empty() && !suffix.contains(['\n', '\r']))
        .map(str::to_owned)
}

fn is_safe_completion(item: &CompletionItem) -> bool {
    item.additional_text_edits
        .as_ref()
        .is_none_or(Vec::is_empty)
        && item.insert_text_format != Some(lsp_types::InsertTextFormat::SNIPPET)
}

pub(in crate::window) fn range_for_snapshot(
    snapshot: &dopad_core::TextSnapshot,
    bytes: std::ops::Range<usize>,
) -> Option<(lsp_types::Range, u32)> {
    use dopad_core::ByteRange;
    let start = snapshot
        .byte_to_position(dopad_core::ByteOffset(bytes.start))
        .ok()?;
    let end = snapshot
        .byte_to_position(dopad_core::ByteOffset(bytes.end))
        .ok()?;
    let start_line = snapshot.line_content_range(start.line).ok()?;
    let end_line = snapshot.line_content_range(end.line).ok()?;
    let start_text = snapshot
        .slice(ByteRange::new(start_line.start.0, bytes.start))
        .ok()?;
    let end_text = snapshot
        .slice(ByteRange::new(end_line.start.0, bytes.end))
        .ok()?;
    let removed = snapshot
        .slice(ByteRange::new(bytes.start, bytes.end))
        .ok()?;
    Some((
        lsp_types::Range::new(
            Position::new(
                u32::try_from(start.line).ok()?,
                u32::try_from(start_text.encode_utf16().count()).ok()?,
            ),
            Position::new(
                u32::try_from(end.line).ok()?,
                u32::try_from(end_text.encode_utf16().count()).ok()?,
            ),
        ),
        u32::try_from(removed.encode_utf16().count()).ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::component::input::Rope;

    #[test]
    fn lsp_positions_use_utf16_columns_and_zero_based_lines() {
        let text = Rope::from("a🙂b\n中x");
        assert_eq!(offset_to_position(&text, "a🙂".len()), Position::new(0, 3));
        assert_eq!(
            offset_to_position(&text, "a🙂b\n中".len()),
            Position::new(1, 1)
        );
    }

    #[test]
    fn inline_completion_only_accepts_plain_safe_suffixes() {
        let plain = CompletionItem {
            label: "print".into(),
            ..Default::default()
        };
        assert_eq!(safe_inline_suffix(&plain, "pri").as_deref(), Some("nt"));
        let snippet = CompletionItem {
            label: "print".into(),
            insert_text_format: Some(lsp_types::InsertTextFormat::SNIPPET),
            ..Default::default()
        };
        assert!(safe_inline_suffix(&snippet, "pri").is_none());
    }

    #[test]
    fn incremental_ranges_convert_grapheme_columns_to_utf16() {
        let snapshot = dopad_core::TextBuffer::new("a🙂b\n中x").snapshot();
        let (range, length) = range_for_snapshot(&snapshot, "a🙂".len().."a🙂b\n中".len())
            .expect("valid UTF-8 range");
        assert_eq!(range.start, Position::new(0, 3));
        assert_eq!(range.end, Position::new(1, 1));
        assert_eq!(length, 3);
    }
}
