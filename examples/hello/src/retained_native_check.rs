//! Opt-in Windows native framebuffer qualification and completed export A/B.
use gpui::*;
use gpui_box::retained::{
    BackendCapabilities, BoxShadow as RetainedShadow, ImageResource, Rect, Style, Transaction,
    UiTree, length, taffy, wgpu::WgpuFrameRenderer,
};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

struct Snapshot {
    surface: WindowsGpuSurface,
    readback: std::sync::Arc<RenderImage>,
    expected: image::RgbaImage,
    at: [u32; 2],
}

struct NativeCheck {
    snapshots: Vec<Snapshot>,
    scale: f32,
    stress: Option<WindowsGpuSurface>,
}

impl NativeCheck {
    fn new(window: &Window, output: &std::path::Path, system: std::sync::Arc<TextSystem>) -> Self {
        let scale = window.scale_factor();
        let mut gpu = WgpuFrameRenderer::new_for_window(window, 3).unwrap();
        assert_eq!(format!("{:?}", gpu.adapter_info().backend), "Dx12");
        let mut tree = fixture();
        attach_text(&mut tree, window, scale, system);
        let root = tree.root();
        let mut snapshots = vec![];
        for (i, dimensions) in [[128, 96], [144, 112], [160, 128]].into_iter().enumerate() {
            let mut tx = Transaction::default();
            tx.color(root, [0x20406080, 0x604020a0, 0x20604060][i]);
            tx.translation(root, [i as f32 * 0.25, i as f32 * 0.5]);
            tree.commit(tx, BackendCapabilities::default()).unwrap();
            let frame = tree.frame(dimensions.map(|v| v as f32 / scale)).unwrap();
            gpu.submit(&frame, dimensions, scale).unwrap();
            let surface = gpu.window_surface().unwrap().expect("native snapshot");
            assert_eq!(surface.size(), dimensions);
            assert!(surface.ptr_eq(&gpu.window_surface().unwrap().unwrap()));
            snapshots.push(Snapshot {
                surface,
                readback: gpu.window_image().unwrap(),
                expected: gpu.image().unwrap(),
                at: [16 + i as u32 * 184, 16],
            });
        }
        // Retain all eight snapshots, reject the ninth before publication, then
        // release only the extra owners and retry the same latest rendered frame.
        let mut extra = vec![];
        for i in 0..5 {
            let mut tx = Transaction::default();
            tx.color(root, 0x12340080 + (i << 16));
            tree.commit(tx, BackendCapabilities::default()).unwrap();
            let frame = tree.frame([64.0 / scale, 48.0 / scale]).unwrap();
            gpu.submit(&frame, [64, 48], scale).unwrap();
            extra.push(gpu.window_surface().unwrap().unwrap());
        }
        let mut tx = Transaction::default();
        tx.color(root, 0x88776680);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = tree.frame([64.0 / scale, 48.0 / scale]).unwrap();
        gpu.submit_for_window(window, &frame, [64, 48], scale)
            .unwrap();
        assert!(gpu.window_surface().is_err());
        drop(extra);
        gpu.window_surface().unwrap().unwrap();
        // Reset preserves DX12/native support; snapshots from the former owner
        // remain independent, even after every producer and source is destroyed.
        gpu.reset().unwrap();
        gpu.submit(&frame, [64, 48], scale).unwrap();
        assert!(gpu.window_surface().unwrap().is_some());
        gpu.finish().unwrap();
        drop((gpu, frame, tree));
        std::fs::write(output.join("fixture.json"), format!("{{\"backend\":\"Dx12\",\"window_scale\":{scale},\"snapshots\":3,\"cache_identity\":true,\"retained_limit_rejected\":true,\"retry\":true,\"reset_preserves_native\":true,\"producer_dropped_before_native_draw\":true}}\n")).unwrap();
        Self {
            snapshots,
            scale,
            stress: None,
        }
    }

    fn validate(&self, actual: &image::RgbaImage) -> Result<usize, String> {
        let mut checked = 0;
        for snapshot in &self.snapshots {
            for extra_y in [0, 176] {
                let [x, mut y] = snapshot.at;
                y += extra_y;
                let (width, height) = snapshot.expected.dimensions();
                for yy in 0..height {
                    for xx in 0..width {
                        let expected = if (38..62).contains(&xx) && (26..46).contains(&yy) {
                            [220, 80, 30, 255]
                        } else if xx >= 8 && xx < width - 8 && yy >= 8 && yy < height - 8 {
                            let pixel = snapshot.expected.get_pixel(xx, yy).0;
                            let mut result = [0, 0, 0, 255];
                            for channel in 0..3 {
                                result[channel] = (u32::from(pixel[channel])
                                    + ([20, 40, 60][channel] * u32::from(255 - pixel[3]) + 127)
                                        / 255)
                                    .min(255)
                                    as u8;
                            }
                            result
                        } else {
                            [20, 40, 60, 255]
                        };
                        let actual = actual.get_pixel(x + xx, y + yy).0;
                        if actual.iter().zip(expected).any(|(a, b)| a.abs_diff(b) > 1) {
                            return Err(format!(
                                "native framebuffer differs at ({},{}): {actual:?} != {expected:?}",
                                x + xx,
                                y + yy
                            ));
                        }
                        checked += 1;
                    }
                }
            }
        }
        Ok(checked)
    }
}

impl Render for NativeCheck {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let surfaces: Vec<_> = self
            .snapshots
            .iter()
            .map(|v| (v.surface.clone(), v.readback.clone(), v.at))
            .collect();
        let scale = self.scale;
        let stress = self.stress.clone();
        canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                let viewport = Bounds {
                    origin: Point::default(),
                    size: window.viewport_size(),
                };
                window.paint_quad(fill(viewport, rgb(0x14283c)));
                for (surface, readback, [x, y]) in surfaces {
                    let [width, height] = surface.size();
                    let rect = |x: f32, y: f32, w: f32, h: f32| Bounds {
                        origin: point(px(x / scale), px(y / scale)),
                        size: size(px(w / scale), px(h / scale)),
                    };
                    let bounds = rect(x as f32, y as f32, width as f32, height as f32);
                    for extra_y in [0, 176] {
                        let y = y + extra_y;
                        let bounds = Bounds {
                            origin: bounds.origin + point(px(0.0), px(extra_y as f32 / scale)),
                            ..bounds
                        };
                        window.with_content_mask(
                            Some(ContentMask {
                                bounds: rect(
                                    (x + 8) as f32,
                                    (y + 8) as f32,
                                    (width - 16) as f32,
                                    (height - 16) as f32,
                                ),
                            }),
                            |window| {
                                if extra_y == 0 {
                                    window.paint_gpu_surface(bounds, surface.clone());
                                } else {
                                    window
                                        .paint_image(
                                            bounds,
                                            bounds,
                                            Corners::default(),
                                            readback.clone(),
                                            0,
                                            false,
                                        )
                                        .unwrap();
                                }
                            },
                        );
                        window.paint_quad(fill(
                            rect((x + 38) as f32, (y + 26) as f32, 24.0, 20.0),
                            rgb(0xdc501e),
                        ));
                    }
                }
                if let Some(surface) = stress {
                    window.paint_gpu_surface(
                        Bounds {
                            origin: point(px(16.0 / scale), px(352.0 / scale)),
                            size: size(px(64.0 / scale), px(48.0 / scale)),
                        },
                        surface,
                    );
                }
            },
        )
        .size_full()
    }
}

fn fixture() -> UiTree {
    let mut tree = UiTree::new(Style::default(), 0x20406080).unwrap();
    let child = tree
        .append(
            tree.root(),
            Style {
                position: taffy::Position::Absolute,
                inset: taffy::Rect {
                    left: length(12.0),
                    top: length(14.0),
                    right: taffy::LengthPercentageAuto::auto(),
                    bottom: taffy::LengthPercentageAuto::auto(),
                },
                size: taffy::Size {
                    width: length(48.0),
                    height: length(36.0),
                },
                ..Default::default()
            },
            0x4080c080,
        )
        .unwrap();
    let mut tx = Transaction::default();
    tx.image(
        child,
        Some(
            ImageResource::rgba8(
                [2, 2],
                vec![
                    255, 20, 80, 128, 20, 240, 60, 255, 40, 70, 230, 190, 220, 150, 30, 100,
                ],
            )
            .unwrap(),
        ),
    );
    tx.shadows(
        child,
        vec![RetainedShadow {
            color: 0x00000080,
            offset: [3.0, 4.0],
            blur_radius: 2.0,
            ..Default::default()
        }],
    );
    tx.opacity(child, 0.65);
    tx.clip(
        tree.root(),
        Some(Rect {
            x: 0.0,
            y: 0.0,
            width: 1024.0,
            height: 1024.0,
        }),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    tree
}

fn attach_text(tree: &mut UiTree, window: &Window, scale: f32, system: std::sync::Arc<TextSystem>) {
    use gpui_box::retained::{
        paragraph::ParagraphSnapshot,
        text::{GlyphRasterizer, TextPaint},
    };
    let paragraph = ParagraphSnapshot::shape(
        window.text_system(),
        "GPU alpha",
        font("Segoe UI"),
        12.0,
        16.0,
    )
    .unwrap();
    let resource = GlyphRasterizer::new(system)
        .prepare(paragraph, scale)
        .unwrap();
    let node = tree
        .append(
            tree.root(),
            Style {
                position: taffy::Position::Absolute,
                inset: taffy::Rect {
                    left: length(6.0),
                    top: length(54.0),
                    right: taffy::LengthPercentageAuto::auto(),
                    bottom: taffy::LengthPercentageAuto::auto(),
                },
                size: taffy::Size {
                    width: length(72.0),
                    height: length(20.0),
                },
                ..Default::default()
            },
            0,
        )
        .unwrap();
    let mut tx = Transaction::default();
    tx.text(
        node,
        Some(TextPaint {
            resource,
            color: 0xffcc8080,
        }),
    );
    tree.commit(tx, BackendCapabilities::default()).unwrap();
}

fn stress(
    window: &mut Window,
    view: &Entity<NativeCheck>,
    cx: &mut App,
    output: &std::path::Path,
) -> Result<(), String> {
    let scale = window.scale_factor();
    let mut tree = UiTree::new(Style::default(), 0).unwrap();
    let mut gpu = WgpuFrameRenderer::new_for_window(window, 3).unwrap();
    for revision in 0..40 {
        let color = [0x80402080, 0x20a040a0, 0x2040c060, 0xc06020ff][revision % 4];
        let mut tx = Transaction::default();
        tx.color(tree.root(), color);
        tree.commit(tx, BackendCapabilities::default()).unwrap();
        let frame = tree.frame([64.0 / scale, 48.0 / scale]).unwrap();
        gpu.submit_for_window(window, &frame, [64, 48], scale)
            .unwrap();
        let surface = gpu.window_surface().unwrap().unwrap();
        view.update(cx, |view, cx| {
            view.stress = Some(surface);
            cx.notify();
        });
        // No producer-side finish/image/poll before native sampling. The native
        // GPU must wait for the export fence, even on the first frame.
        window.draw(cx).clear(cx);
        let image = window.render_to_image().map_err(|e| e.to_string())?;
        let expected = gpu.image().unwrap();
        for y in 0..48 {
            for x in 0..64 {
                let source = expected.get_pixel(x, y).0;
                let actual = image.get_pixel(x + 16, y + 352).0;
                for channel in 0..3 {
                    let expected = (u32::from(source[channel])
                        + ([20, 40, 60][channel] * u32::from(255 - source[3]) + 127) / 255)
                        .min(255) as u8;
                    if actual[channel].abs_diff(expected) > 1 {
                        return Err(format!(
                            "native reuse differs at revision {revision}, {x},{y}: {actual:?}, {source:?}"
                        ));
                    }
                }
            }
        }
        if revision == 39 {
            image
                .save(output.join("native-reuse-framebuffer.png"))
                .map_err(|e| e.to_string())?;
        }
    }
    let stats = gpu.native_surface_stats().unwrap();
    if stats.live_allocations > 4 || stats.reuses < 36 {
        return Err(format!(
            "native pool failed to reuse completed reads: {stats:?}"
        ));
    }
    std::fs::write(output.join("native-reuse.json"),format!("{{\"frames\":40,\"pixels\":122880,\"producer_wait_before_native_draw\":false,\"allocations\":{},\"reuses\":{},\"copies\":{},\"live_allocations\":{}}}\n",stats.allocations,stats.reuses,stats.copies,stats.live_allocations)).unwrap();
    Ok(())
}

fn recover_native_device(
    window: &mut Window,
    view: &Entity<NativeCheck>,
    cx: &mut App,
    output: &std::path::Path,
) -> Result<usize, String> {
    let scale = window.scale_factor();
    let mut tree = UiTree::new(Style::default(), 0x20406080).unwrap();
    let mut gpu = WgpuFrameRenderer::new_for_window(window, 3).unwrap();
    let frame = tree.frame([64.0 / scale, 48.0 / scale]).unwrap();
    gpu.submit_for_window(window, &frame, [64, 48], scale)
        .unwrap();
    let old_surface = gpu.window_surface().unwrap().unwrap();
    let old_device = window.gpu_surface_device().unwrap();
    if !old_surface.belongs_to(&old_device) {
        return Err("initial native surface belongs to the wrong device".into());
    }

    // The diagnostic scene still owns snapshots from the old device. Drop those
    // views before exercising the replacement; old producer snapshots retained
    // only by the caller must remain valid but must not be submitted to a new
    // native scene.
    view.update(cx, |view, _| {
        view.snapshots.clear();
        view.stress = None;
    });
    window
        .force_gpu_device_replacement_for_test()
        .map_err(|e| e.to_string())?;
    let new_device = window.gpu_surface_device().unwrap();
    if old_surface.belongs_to(&new_device) {
        return Err("native device replacement retained the old D3D11 identity".into());
    }

    let mut tx = Transaction::default();
    tx.color(tree.root(), 0x80c040c0);
    tree.commit(tx, BackendCapabilities::default()).unwrap();
    let frame = tree.frame([64.0 / scale, 48.0 / scale]).unwrap();
    let submission = gpu
        .submit_for_window(window, &frame, [64, 48], scale)
        .unwrap()
        .ok_or_else(|| "recovered frame unexpectedly skipped submission".to_owned())?;
    if !submission.full_resync {
        return Err("recovered frame did not perform a full GPU resync".into());
    }
    if gpu.native_recoveries() != 1 || gpu.device_epoch() != 1 {
        return Err(format!(
            "native recovery epoch was not advanced: recoveries={}, epoch={}",
            gpu.native_recoveries(),
            gpu.device_epoch()
        ));
    }
    let surface = gpu.window_surface().unwrap().unwrap();
    if !surface.belongs_to(&new_device) || surface.ptr_eq(&old_surface) {
        return Err("recovered native surface was not recreated for the new device".into());
    }
    view.update(cx, |view, cx| {
        view.stress = Some(surface);
        cx.notify();
    });
    window.draw(cx).clear(cx);
    let image = window.render_to_image().map_err(|e| e.to_string())?;
    let expected = gpu.image().unwrap();
    let mut checked = 0;
    for y in 0..48 {
        for x in 0..64 {
            let source = expected.get_pixel(x, y).0;
            let actual = image.get_pixel(x + 16, y + 352).0;
            for channel in 0..3 {
                let expected = (u32::from(source[channel])
                    + ([20, 40, 60][channel] * u32::from(255 - source[3]) + 127) / 255)
                    .min(255) as u8;
                if actual[channel].abs_diff(expected) > 1 {
                    return Err(format!(
                        "recovered native surface differs at {x},{y}: {actual:?}, {source:?}"
                    ));
                }
            }
            checked += 1;
        }
    }
    // A repeated normal-window submit must reuse the new epoch without another
    // platform recovery.
    if gpu
        .submit_for_window(window, &frame, [64, 48], scale)
        .unwrap()
        .is_some()
    {
        return Err("equal frame caused redundant GPU submission".into());
    }
    if gpu.native_recoveries() != 1 || gpu.device_epoch() != 1 {
        return Err("stable device identity triggered a redundant recovery".into());
    }
    image
        .save(output.join("native-device-recovery.png"))
        .map_err(|e| e.to_string())?;
    std::fs::write(
        output.join("native-device-recovery.json"),
        format!(
            "{{\"passed\":true,\"pixels\":{checked},\"device_identity_changed\":true,\"old_snapshot_retained\":true,\"full_resync\":true,\"recoveries\":{},\"device_epoch\":{},\"redundant_recovery\":false}}\n",
            gpu.native_recoveries(),
            gpu.device_epoch()
        ),
    )
    .unwrap();
    Ok(checked)
}

fn benchmark(window: &Window, output: &std::path::Path) {
    let mut observations = String::new();
    std::fs::write(
        output.join("export-adapter.txt"),
        format!(
            "{:?}\n",
            WgpuFrameRenderer::new_for_window(window, 3)
                .unwrap()
                .adapter_info()
        ),
    )
    .unwrap();
    let mut lines = String::from(
        "width,height,round,position,mode,sample,commit_submit_us,export_complete_us,total_us\n",
    );
    let mut gpu = WgpuFrameRenderer::new_for_window(window, 3).unwrap();
    let mut tree = fixture();
    for dimensions in [[128, 96], [800, 600], [1600, 900]] {
        let viewport = dimensions.map(|v| v as f32);
        let root = tree.root();
        for round in 0..4 {
            for position in 0..2 {
                let native = (round + position) % 2 == 0;
                for sample in 0..26 {
                    let start = Instant::now();
                    let mut tx = Transaction::default();
                    tx.color(
                        root,
                        if sample % 2 == 0 {
                            0x204060ff
                        } else {
                            0x604020ff
                        },
                    );
                    tree.commit(tx, BackendCapabilities::default()).unwrap();
                    let frame = tree.frame(viewport).unwrap();
                    gpu.submit(&frame, dimensions, 1.0).unwrap();
                    let commit_submit_us = start.elapsed().as_secs_f64() * 1e6;
                    let export_start = Instant::now();
                    if native {
                        let _ = gpu.window_surface().unwrap().unwrap();
                    } else {
                        let _ = gpu.window_image().unwrap();
                    }
                    gpu.finish().unwrap();
                    let export_complete_us = export_start.elapsed().as_secs_f64() * 1e6;
                    let total_us = start.elapsed().as_secs_f64() * 1e6;
                    if sample >= 6 {
                        lines.push_str(&format!("{},{},{round},{position},{},{},{commit_submit_us:.3},{export_complete_us:.3},{total_us:.3}\n",dimensions[0],dimensions[1],if native {"gpu"} else {"readback"},sample-6));
                    }
                }
            }
        }
    }
    let stats = gpu.native_surface_stats().unwrap();
    observations.push_str(&format!("allocations={},reuses={},copies={},cache_hits={},copied_bytes={},live_allocations={},live_payload_bytes={}\n",stats.allocations,stats.reuses,stats.copies,stats.cache_hits,stats.copied_bytes,stats.live_allocations,stats.live_payload_bytes));
    std::fs::write(output.join("export-pool-stats.txt"), observations).unwrap();
    std::fs::write(output.join("export-benchmark.csv"), lines).unwrap();
}

/// Runs a diagnostic in a real native window and writes raw evidence.
pub fn open(cx: &mut App, output: PathBuf) {
    std::fs::create_dir_all(&output).unwrap();
    let window = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds {
                    origin: Point::default(),
                    size: size(px(680.0), px(420.0)),
                })),
                ..Default::default()
            },
            |window, cx| cx.new(|cx| NativeCheck::new(window, &output, cx.text_system().clone())),
        )
        .unwrap();
    let view = window.update(cx, |_, _, cx| cx.entity()).unwrap();
    cx.activate(true);
    cx.spawn(async move |cx| {
        cx.background_executor().timer(Duration::from_millis(300)).await;
        let result=AnyWindowHandle::from(window).update(cx, |_, window, cx| {
            // Native framebuffer validation is explicit and outside measurements.
            window.draw(cx).clear(cx);
            let image=window.render_to_image().map_err(|e|e.to_string())?;
            image.save(output.join("native-framebuffer.png")).map_err(|e|e.to_string())?;
            let checked=view.read(cx).validate(&image)?;
            std::fs::write(output.join("native-window.json"),format!("{{\"visible\":{},\"explicit_scene_draw\":true,\"framebuffer_readback_for_validation\":true,\"os_present_latency_measured\":false}}\n",window.visibility().is_visible())).unwrap();
            // Replaying a cached scene must continue to work without producers.
            let repeat=window.render_to_image().map_err(|e|e.to_string())?;
            if image.as_raw()!=repeat.as_raw() {return Err("cached native scene changed".to_owned());}
            let recovery_pixels = recover_native_device(window, &view, cx, &output)?;
            stress(window,&view,cx,&output)?;
            benchmark(window,&output);
            Ok((checked, recovery_pixels))
        });
        let report=match result {Ok(Ok((checked, recovery_pixels)))=>format!("{{\"passed\":true,\"native_framebuffer_pixels\":{checked},\"channel_tolerance\":1,\"cached_scene_equal\":true,\"device_replacement_pixels\":{recovery_pixels},\"device_replacement_passed\":true,\"benchmark_scope\":\"owner commit/publication/submission plus completed export; same DX12 consumer, full redraw; native draw/present and sprite upload excluded; GPU export reuses only unowned allocations after producer and native read fence completion\"}}\n"),other=>format!("{{\"passed\":false,\"error\":{:?}}}\n",format!("{other:?}"))};
        std::fs::write(output.join("result.json"),report).unwrap();
        cx.update(|cx|cx.quit());
    }).detach();
}
