//! Isolated CPU property publication; no GPU, compilation or presentation timing.
use gpui_box::retained::*;
use std::error::Error;
mod retained_properties;

fn main() -> Result<(), Box<dyn Error>> {
    let output = std::env::args().nth(1).expect("output JSON path");
    std::fs::write(
        output,
        serde_json::to_vec_pretty(&retained_properties::benchmark()?)?,
    )?;
    Ok(())
}
