//! SQL queries loaded from `sql/*.sql` files via `include_str!`.

pub const SCHEMA: &str = include_str!("../../sql/schema.sql");
pub const TOUCH_ACCESS: &str = include_str!("../../sql/touch_access.sql");
pub const SELECT_VALUE: &str = include_str!("../../sql/select_value.sql");
pub const SELECT_ENTRIES: &str = include_str!("../../sql/select_entries.sql");
pub const UPSERT: &str = include_str!("../../sql/upsert.sql");
pub const DELETE: &str = include_str!("../../sql/delete.sql");
pub const UPSERT_FULL: &str = include_str!("../../sql/upsert_full.sql");
pub const SELECT_ENTRY: &str = include_str!("../../sql/select_entry.sql");
pub const RECALL_FTS: &str = include_str!("../../sql/recall_fts.sql");
pub const RECALL_VECTOR: &str = include_str!("../../sql/recall_vector.sql");
