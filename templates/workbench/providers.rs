//! Explicit development provider, replaced by the application's service.
use crate::contracts::{DocumentSummary, Documents};
use std::{cell::RefCell, collections::BTreeMap};

pub struct FakeDocuments {
    entries: RefCell<BTreeMap<String, (String, String)>>,
}

impl FakeDocuments {
    pub fn new() -> Self {
        Self {
            entries: RefCell::new(
                [
                    ("welcome", "Welcome", "Edit this in-memory document."),
                    (
                        "components",
                        "Shared components",
                        "Compose controls across desktop and Android.",
                    ),
                    (
                        "services",
                        "Platform services",
                        "Inject explicit providers through narrow contracts.",
                    ),
                ]
                .into_iter()
                .map(|(id, title, text)| (id.into(), (title.into(), text.into())))
                .collect(),
            ),
        }
    }
}

impl Documents for FakeDocuments {
    fn list(&self) -> Vec<DocumentSummary> {
        self.entries
            .borrow()
            .iter()
            .map(|(id, (title, _))| (id.clone(), title.clone()))
            .collect()
    }

    fn read(&self, id: &str) -> Result<String, String> {
        self.entries
            .borrow()
            .get(id)
            .map(|(_, text)| text.clone())
            .ok_or_else(|| "Document is unavailable".into())
    }

    fn save(&self, id: &str, text: &str) -> Result<(), String> {
        if text.len() > 64 * 1024 {
            return Err("Document exceeds the 64 KiB preview limit".into());
        }
        let mut entries = self.entries.borrow_mut();
        let (_, content) = entries.get_mut(id).ok_or("Document is unavailable")?;
        *content = text.to_owned();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_documents_preserve_text_and_reject_missing_or_oversized_writes() {
        let provider = FakeDocuments::new();
        assert_eq!(provider.list().len(), 3);
        provider.save("welcome", "中文 🦀").unwrap();
        assert_eq!(provider.read("welcome").unwrap(), "中文 🦀");
        assert!(provider.read("missing").is_err());
        assert!(provider.save("missing", "value").is_err());
        assert!(provider.save("welcome", &"x".repeat(65537)).is_err());
        assert_eq!(provider.read("welcome").unwrap(), "中文 🦀");
    }
}
