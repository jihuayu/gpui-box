//! Feature-facing contracts. This module has no GPUI or feature dependencies.
pub type DocumentSummary = (String, String);

pub trait Documents {
    fn list(&self) -> Vec<DocumentSummary>;
    fn read(&self, id: &str) -> Result<String, String>;
    fn save(&self, id: &str, text: &str) -> Result<(), String>;
}
