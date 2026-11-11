//! Command-palette query, deterministic ranking and projected entries.

use crate::CommandId;

/// Maximum number of palette entries returned by one query.
pub const MAX_PALETTE_LIMIT: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteQuery {
    pub text: String,
    pub category: Option<String>,
    pub limit: usize,
}

impl Default for PaletteQuery {
    fn default() -> Self {
        Self {
            text: String::new(),
            category: None,
            limit: 50,
        }
    }
}

impl PaletteQuery {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }

    pub fn with_category(mut self, category: impl Into<String>) -> Self {
        self.category = Some(category.into());
        self
    }

    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = limit.min(MAX_PALETTE_LIMIT);
        self
    }
}

/// One palette result. `enabled` and `disabled_reason` are projections only;
/// invocation re-validates live state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaletteEntry {
    pub command_id: CommandId,
    pub title: String,
    pub category: Option<String>,
    pub summary: Option<String>,
    pub score: u32,
    pub shortcut_hint: Option<String>,
    pub enabled: bool,
    pub disabled_reason: Option<String>,
}

/// Deterministic fuzzy/prefix/substring score.
///
/// Higher is better. `None` means the title and identity do not match at all.
pub fn palette_score(query: &str, title: &str, command: &CommandId) -> Option<u32> {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return Some(0);
    }
    let title = title.to_ascii_lowercase();
    if title == query {
        return Some(1000);
    }
    if title.starts_with(&query) {
        let remaining = title.len().saturating_sub(query.len()).min(99) as u32;
        return Some(900 - remaining);
    }
    if let Some(position) = title.find(&query) {
        let penalty = (position as u32).min(100);
        return Some(700 - penalty);
    }
    let identity = command.as_str().to_ascii_lowercase();
    if identity.contains(&query) {
        return Some(500);
    }
    fuzzy_score(&title, &query).map(|score| 300 + score)
}

fn fuzzy_score(haystack: &str, needle: &str) -> Option<u32> {
    let needle: Vec<char> = needle.chars().collect();
    if needle.is_empty() {
        return Some(0);
    }
    let haystack: Vec<char> = haystack.chars().collect();
    let mut matched = 0;
    let mut last = None;
    let mut gaps = 0_u32;
    for (index, character) in haystack.into_iter().enumerate() {
        if matched < needle.len() && character == needle[matched] {
            if let Some(previous) = last {
                gaps = gaps.saturating_add((index - previous - 1) as u32);
            }
            last = Some(index);
            matched += 1;
        }
    }
    if matched != needle.len() {
        return None;
    }
    Some(100_u32.saturating_sub(gaps.min(100)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: &str) -> CommandId {
        CommandId::new(value).expect("id")
    }

    #[test]
    fn ranking_prefers_exact_then_prefix_then_substring_then_fuzzy() {
        let command = id("file.save");
        let exact = palette_score("save", "Save", &command).expect("exact");
        let prefix = palette_score("sa", "Save", &command).expect("prefix");
        let substring = palette_score("ave", "Save", &command).expect("substring");
        let fuzzy = palette_score("sve", "Save", &command).expect("fuzzy");
        assert!(exact > prefix);
        assert!(prefix > substring);
        assert!(substring > fuzzy);
        assert_eq!(palette_score("zzz", "Save", &command), None);
        assert_eq!(palette_score("", "Save", &command), Some(0));
    }

    #[test]
    fn query_limit_is_bounded() {
        let query = PaletteQuery::new("save").with_limit(usize::MAX);
        assert_eq!(query.limit, MAX_PALETTE_LIMIT);
    }
}
