//! App-internal diagnostic in a real native window, not injected OS input.
use super::*;
use std::{path::PathBuf, time::Duration};

pub(crate) fn open(cx: &mut App, output: PathBuf, cpu: bool) {
    std::fs::create_dir_all(&output).unwrap();
    cx.text_system()
        .add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!(
            "../../assets/NotoSansSC-Regular.otf"
        ))])
        .unwrap();
    let window = cx
        .open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    Point::default(),
                    size(px(600.0), px(460.0)),
                ))),
                ..Default::default()
            },
            |window, cx| cx.new(|cx| OwnerDemo::new(window, cx, cpu)),
        )
        .unwrap();
    let view = window.update(cx, |_, _, cx| cx.entity()).unwrap();
    cx.activate(true);
    cx.spawn(async move |cx| {
        let result: Result<String, String> = async {
            // Diagnostic pacing only. Production has no timer/polling loop.
            cx.background_executor().timer(Duration::from_millis(200)).await;
            let mut evidence = String::new();
            view.read_with(cx, |this,_| {
                evidence.push_str(&format!("adapter={:?}\n",this.gpu.as_ref().map(|gpu|gpu.adapter_info())));
            });
            for stage in 0..6 {
                for _ in 0..200 {
                    AnyWindowHandle::from(window).update(cx, |_, window, cx| window.draw(cx).clear(cx)).map_err(|e|e.to_string())?;
                    cx.background_executor().timer(Duration::from_millis(25)).await;
                    if view.read_with(cx, |view, _| view.cpu_task.is_none() && view.pending.is_none() && view.ready.is_none() && view.shown.is_some() && !view.animation.as_ref().is_some_and(AnimationRuntime::is_animating)) { break; }
                }
                let (pixels, count, scroll, native, scale) = AnyWindowHandle::from(window).update(cx, |_, window, cx| {
                    window.draw(cx).clear(cx);
                    window.present_if_needed();
                    let this = view.read(cx);
                    if let Some(error) = &this.error { return Err(error.clone()); }
                    if this.cpu_task.is_some() || this.pending.is_some() || this.ready.is_some() || this.animation.as_ref().is_some_and(AnimationRuntime::is_animating) { return Err("render work not settled".into()); }
                    let shown = this.shown.as_ref().ok_or("no installed image")?;
                    if this.router.scene_id() != Some(shown.scene.id()) { return Err("installed input identity differs".into()); }
                    if this.animate {
                        if shown.scene.id().presentation.is_none() {return Err("animation image has no sampled input identity".into());}
                        let expected=if (*this.ui.read(this.count).unwrap()).is_multiple_of(2) {0.0} else {120.0};
                        if shown.frame.resolve_geometry(this.animated_button).unwrap().spatial.translation!=[expected,0.0] {return Err("animation final placement differs".into());}
                    }
                    let mut job = SoftwareJob::new(shown.frame.resolve(), SoftwareOptions {
                        size: shown.size, scale: shown.scale, background: 0,
                        effects: pacing::SoftwareEffects::Exact, max_bytes: 128*1024*1024,
                    }).map_err(|e|format!("{e:?}"))?;
                    while !job.step(8192).map_err(|e|format!("{e:?}"))? {}
                    let reference = job.into_frame().map_err(|e|format!("{e:?}"))?;
                    let image = window.render_to_image().map_err(|e|e.to_string())?;
                    let bounds = this.bounds.unwrap();
                    let origin = [f32::from(bounds.origin.x), f32::from(bounds.origin.y)].map(|v|(v*shown.scale).round() as u32);
                    let mut checked = 0;
                    for y in 0..shown.size[1] { for x in 0..shown.size[0] {
                        let expected = &reference.pixels()[((y*shown.size[0]+x)*4) as usize..][..4];
                        let actual = image.get_pixel(origin[0]+x, origin[1]+y).0;
                        if actual.iter().zip(expected).any(|(a,b)| a.abs_diff(*b)>1) {
                            return Err(format!("native/CPU reference differ stage {stage} pixel {x},{y}: {actual:?} / {expected:?}"));
                        }
                        checked+=1;
                    }}
                    image.save(output.join(format!("stage-{stage}.png"))).map_err(|e|e.to_string())?;
                    let scene = this.ui.input_scene().map_err(|e|e.to_string())?;
                    let scroll = scene.scroll_state(scene.node_id(this.scroll_host).unwrap()).unwrap().offset[1];
                    #[cfg(target_os = "windows")]
                    let native = matches!(shown.raster, Raster::Gpu(_));
                    #[cfg(not(target_os = "windows"))]
                    let native = false;
                    Ok((checked, *this.ui.read(this.count).unwrap(), scroll, native, shown.scale))
                }).map_err(|e|e.to_string())??;
                let expected = [0,1,2,12,12,12][stage];
                if count != expected { return Err(format!("stage {stage}: count {count}, expected {expected}")); }
                if stage==3 && scroll<=0.0 { return Err("focus did not reveal bottom button".into()); }
                evidence.push_str(&format!("stage={stage},count={count},scroll={scroll},pixels={pixels},native_surface={native},scale={scale},animate={}\n",std::env::args().any(|arg|arg=="--animate")));
                AnyWindowHandle::from(window).update(cx, |_, window, cx| {
                    let click = {
                        let this=view.read(cx);
                        let shown=this.shown.as_ref().unwrap();
                        let item=shown.frame.resolve_geometry(this.buttons[0]).unwrap();
                        this.bounds.unwrap().origin+point(px(item.bounds.x+20.0),px(item.bounds.y+20.0))
                    };
                    let key=|name|Keystroke::parse(name).unwrap();
                    match stage {
                        0 => {
                            window.dispatch_event(MouseDownEvent{position:click,button:MouseButton::Left,..Default::default()}.to_platform_input(),cx);
                            window.dispatch_event(MouseUpEvent{position:click,button:MouseButton::Left,..Default::default()}.to_platform_input(),cx);
                        }
                        1 => {
                            window.dispatch_keystroke(key("enter"),cx);
                            window.dispatch_event(KeyUpEvent{keystroke:key("enter")}.to_platform_input(),cx);
                        }
                        2 => {
                            window.dispatch_keystroke(key("tab"),cx);
                            window.dispatch_keystroke(key("space"),cx);
                            window.dispatch_event(KeyUpEvent{keystroke:key("space")}.to_platform_input(),cx);
                        }
                        3 => {
                            let p=view.read(cx).bounds.unwrap().origin+point(px(60.0),px(170.0));
                            window.dispatch_event(ScrollEvent{position:p,delta:ScrollDelta::Pixels(point(px(0.0),px(60.0))),..Default::default()}.to_platform_input(),cx);
                        }
                        4 => {
                            window.dispatch_event(MouseDownEvent{position:click,button:MouseButton::Left,..Default::default()}.to_platform_input(),cx);
                            let outside=point(click.x,px(0.0));
                            window.dispatch_event(MouseMoveEvent{position:outside,pressed_button:Some(MouseButton::Left),..Default::default()}.to_platform_input(),cx);
                            window.dispatch_event(MouseUpEvent{position:outside,button:MouseButton::Left,..Default::default()}.to_platform_input(),cx);
                        }
                        _=>{}
                    }
                }).map_err(|e|e.to_string())?;
            }
            view.read_with(cx, |this,_| -> Result<(),String> {
                if this.router.captured().is_some() {Err("capture not released".into())}else{Ok(())}
            })?;
            Ok(evidence)
        }.await;
        match result {
            Ok(evidence)=>std::fs::write(output.join("result.txt"),format!("passed=true\nmode={}\ninput=app-internal GPUI dispatch, not OS injection\nframebuffer=explicit native draw/readback, not displayed cadence\n{evidence}",if cpu{"CPU"}else{"GPU"})).unwrap(),
            Err(error)=>std::fs::write(output.join("result.txt"),format!("passed=false\n{error}\n")).unwrap(),
        }
        cx.update(|cx|cx.quit());
    }).detach();
}
