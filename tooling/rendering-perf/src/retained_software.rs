//! Actual CPU pixels against the original/fresh WGPU controls; no readback is
//! used to construct CPU output. GPU readback is validation-only.
use super::*;
use gpui_box::retained::{pacing::SoftwareEffects, software::*};

fn absolute(x: f32, y: f32, w: f32, h: f32) -> Style {
    Style {
        position: taffy::Position::Absolute,
        inset: taffy::Rect {
            left: length(x),
            top: length(y),
            right: taffy::LengthPercentageAuto::auto(),
            bottom: taffy::LengthPercentageAuto::auto(),
        },
        size: taffy::Size {
            width: length(w),
            height: length(h),
        },
        ..Default::default()
    }
}
pub(super) fn run(
    renderer: &mut WgpuFrameRenderer,
    output: &std::path::Path,
    results: &mut Vec<serde_json::Value>,
) -> Result<(), Box<dyn Error>> {
    for scale in [1.0, 1.25, 1.5, 2.0] {
        for case in [
            "rectangles",
            "isolated-opacity",
            "image",
            "rounded",
            "fractional",
            "text",
        ] {
            let mut tree = UiTree::new(absolute(0.0, 0.0, 64.0, 48.0), 0x202020ff)?;
            let a = tree.append(tree.root(), absolute(8.0, 8.0, 32.0, 32.0), 0xff0000ff)?;
            let b = tree.append(a, absolute(8.0, 8.0, 16.0, 16.0), 0x0000ffff)?;
            let mut tx = Transaction::default();
            let (context, mut raster) = retained_text::context()?;
            match case {
                "isolated-opacity" => {
                    tx.opacity(a, 0.5);
                    tx.opacity(b, 0.5);
                }
                "image" => tx.image(
                    b,
                    Some(ImageResource::rgba8(
                        [2, 2],
                        vec![
                            0, 255, 0, 255, 0, 0, 255, 128, 255, 0, 0, 128, 255, 255, 255, 255,
                        ],
                    )?),
                ),
                "rounded" => tx.decoration(
                    a,
                    QuadDecoration {
                        radius: 6.0,
                        border_width: 2.0,
                        border_color: 0x00ff00ff,
                    },
                ),
                "text" => tx.text(
                    b,
                    Some(text::TextPaint {
                        resource: retained_text::text(&context, &mut raster, "A", scale)?,
                        color: 0xffffffff,
                    }),
                ),
                "fractional" => {
                    tx.translation(a, [-0.25, 0.5]);
                    tx.clip(
                        a,
                        Some(Rect {
                            x: 0.25,
                            y: 0.25,
                            width: 20.5,
                            height: 20.5,
                        }),
                    );
                }
                _ => (),
            }
            tree.commit(tx, BackendCapabilities::default())?;
            let frame = tree.frame([64.0, 48.0])?;
            let size = [(64.0 * scale) as u32, (48.0 * scale) as u32];
            let mut job = SoftwareJob::new(
                frame.clone(),
                SoftwareOptions {
                    size,
                    scale,
                    background: 0,
                    effects: SoftwareEffects::Exact,
                    max_bytes: 32 * 1024 * 1024,
                },
            )
            .map_err(|e| format!("CPU init: {e:?}"))?;
            while !job.step(512).map_err(|e| format!("CPU work: {e:?}"))? {}
            let software = job
                .into_frame()
                .map_err(|e| format!("CPU publish: {e:?}"))?;
            let gpu = renderer.reference_image(&frame, size, scale)?;
            let mut different = 0usize;
            let mut max_error = 0u8;
            for (a, b) in software
                .pixels()
                .as_chunks::<4>()
                .0
                .iter()
                .zip(gpu.as_raw().as_chunks::<4>().0)
            {
                let error = a.iter().zip(b).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
                max_error = max_error.max(error);
                different += usize::from(error > 1);
            }
            // Fractional AA is explicitly experimental. Integer rects/group
            // opacity and flat image samples are the initial qualified subset.
            let qualified = matches!(case, "rectangles" | "isolated-opacity" | "image" | "text");
            if qualified {
                assert_eq!(
                    different, 0,
                    "CPU {case} scale {scale}, max error {max_error}"
                );
            }
            gpu.save(output.join(format!("software-{case}-{scale}-gpu.png")))?;
            // Use the GPU image container only for PNG encoding, never raster.
            let mut encoded = gpu.clone();
            encoded.as_mut().copy_from_slice(software.pixels());
            encoded.save(output.join(format!("software-{case}-{scale}-cpu.png")))?;
            results.push(serde_json::json!({"case":format!("software-{case}-{scale}"),"pixel_equal":different==0,"qualified_subset":qualified,"channel_tolerance":1,"pixels_above_tolerance":different,"max_channel_error":max_error,"cpu_raster":true,"gpu_readback_validation_only":true,"work_units":software.stats.work_units,"samples":software.stats.raster_samples}));
        }
    }
    Ok(())
}
