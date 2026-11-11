//! CPU layout/publication comparison, independent of GPU work.
use gpui_box::retained::*;
use std::error::Error;
mod retained_layout;
fn main() -> Result<(), Box<dyn Error>> {
    let output = std::env::args().nth(1).expect("output JSON path");
    std::fs::write(
        output,
        serde_json::to_vec_pretty(&retained_layout::benchmark()?)?,
    )?;
    Ok(())
}
