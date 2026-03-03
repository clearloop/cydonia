//! Menu abstractions for channel bot commands and keyboards.
//!
//! Platform adapters map these generic types to their native UI elements
//! (e.g. Telegram inline keyboards, slash commands).

use compact_str::CompactString;

/// A bot command that users can invoke (e.g. `/start`, `/help`).
#[derive(Debug, Clone)]
pub struct BotCommand {
    /// Command name without the leading slash (e.g. `"start"`).
    pub name: CompactString,
    /// Short description shown in the command menu.
    pub description: CompactString,
}

/// A button in an inline keyboard row.
#[derive(Debug, Clone)]
pub struct InlineButton {
    /// Button label text.
    pub label: CompactString,
    /// Callback data sent when the button is pressed.
    pub callback: CompactString,
}

/// A row of inline buttons.
pub type InlineRow = Vec<InlineButton>;

/// An inline keyboard attached to a message.
#[derive(Debug, Clone, Default)]
pub struct InlineKeyboard {
    /// Rows of buttons.
    pub rows: Vec<InlineRow>,
}

impl InlineKeyboard {
    /// Create an empty keyboard.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a row of buttons.
    pub fn row(mut self, buttons: Vec<InlineButton>) -> Self {
        self.rows.push(buttons);
        self
    }
}
