//! Tool dispatch handlers for remember, recall, and compact.

use crate::hook::memory::{MemoryHook, MemoryTarget, RecallInput, RememberInput, lance::EntryRow};

impl MemoryHook {
    /// Dispatch the `remember` tool call.
    pub(crate) async fn dispatch_remember(&self, args: &str, agent: &str) -> String {
        let input: RememberInput = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return format!("invalid arguments: {e}"),
        };
        if input.key.is_empty() {
            return "missing required field: key".to_owned();
        }

        match input.target {
            MemoryTarget::Soul => self.write_soul(&input.key, &input.value, agent).await,
            MemoryTarget::User => self.write_user(&input.key, &input.value).await,
            MemoryTarget::Store => {
                self.store_lance(&input.key, &input.value, agent, "fact")
                    .await
            }
        }
    }

    /// Dispatch the `recall` tool call.
    pub(crate) async fn dispatch_recall(&self, args: &str, agent: &str) -> String {
        let input: RecallInput = match serde_json::from_str(args) {
            Ok(v) => v,
            Err(e) => return format!("invalid arguments: {e}"),
        };
        if input.query.is_empty() {
            return "missing required field: query".to_owned();
        }
        let limit = input.limit.unwrap_or(10) as usize;

        match self.lance.search_by_agent(&input.query, agent, limit).await {
            Ok(entries) if entries.is_empty() => "no memories found".to_owned(),
            Ok(entries) => entries
                .iter()
                .map(|e| format!("{}: {}", e.key, e.value))
                .collect::<Vec<_>>()
                .join("\n"),
            Err(e) => format!("recall failed: {e}"),
        }
    }

    /// Dispatch the `compact` tool call.
    ///
    /// Returns an acknowledgement. Actual compaction is driven by the runtime
    /// which calls `Hook::on_compact` separately after this tool returns.
    pub(crate) fn dispatch_compact(&self) -> String {
        "compact acknowledged — context compaction will be triggered by the runtime".to_owned()
    }

    /// Write a key-value pair to SOUL.md for the given agent.
    async fn write_soul(&self, key: &str, value: &str, agent: &str) -> String {
        let dir = self.memory_dir.join(agent);
        if let Err(e) = tokio::fs::create_dir_all(&dir).await {
            return format!("failed to create agent dir: {e}");
        }
        let path = dir.join("SOUL.md");
        let content: String = tokio::fs::read_to_string(&path).await.unwrap_or_default();

        // Append or update the key in markdown format.
        let entry = format!("- **{key}**: {value}");
        let marker = format!("- **{key}**:");
        let new_content = if let Some(pos) = content.find(&marker) {
            let end = content[pos..]
                .find('\n')
                .map(|i| pos + i + 1)
                .unwrap_or(content.len());
            format!("{}{entry}\n{}", &content[..pos], &content[end..])
        } else if content.is_empty() {
            format!("# Soul\n\n{entry}\n")
        } else {
            format!("{content}{entry}\n")
        };

        match tokio::fs::write(&path, &new_content).await {
            Ok(()) => format!("remembered (soul): {key}"),
            Err(e) => format!("failed to write SOUL.md: {e}"),
        }
    }

    /// Write a key-value pair to the global User.toml.
    async fn write_user(&self, key: &str, value: &str) -> String {
        let path = self.memory_dir.join("User.toml");
        let content: String = tokio::fs::read_to_string(&path).await.unwrap_or_default();

        let mut doc = content
            .parse::<toml_edit::DocumentMut>()
            .unwrap_or_default();
        doc[key] = toml_edit::value(value);

        match tokio::fs::write(&path, doc.to_string()).await {
            Ok(()) => format!("remembered (user): {key}"),
            Err(e) => format!("failed to write User.toml: {e}"),
        }
    }

    /// Store a key-value pair in LanceDB.
    async fn store_lance(&self, key: &str, value: &str, agent: &str, entry_type: &str) -> String {
        let row = EntryRow {
            key,
            value,
            agent,
            entry_type,
        };
        match self.lance.upsert(&row).await {
            Ok(()) => format!("remembered (store): {key}"),
            Err(e) => format!("failed to store: {e}"),
        }
    }
}
