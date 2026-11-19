//! Native window qualification: observe normal scheduled draw/Present during a
//! blocked component/Flex owner. Explicit readback is only used outside stalls.
use super::*;
use std::{collections::BTreeSet, path::PathBuf};

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
            |w, cx| cx.new(|cx| SplitDemo::new(w, cx, cpu)),
        )
        .unwrap();
    let view = window.update(cx, |_, _, cx| cx.entity()).unwrap();
    cx.activate(true);
    cx.spawn(async move|cx| {
        let result:Result<String,String>=async {
            let mut report=String::new();
            for _ in 0..400 {
                AnyWindowHandle::from(window).update(cx,|_,w,cx|w.draw(cx).clear(cx)).map_err(|e|e.to_string())?;
                cx.background_executor().timer(Duration::from_millis(10)).await;
                if view.read_with(cx,|this,_|this.shown.is_some()&&this.outstanding==0&&this.cpu_task.is_none()) {break;}
            }
            view.read_with(cx, |this, _| {
                if let Some(e) = &this.error { return Err(e.clone()); }
                if this.shown.is_none() { return Err("no initial complete picture".into()); }
                Ok(())
            })?;
            let visible=AnyWindowHandle::from(window).update(cx,|_,w,_|w.is_visible()).map_err(|e|e.to_string())?;
            if !visible {return Err("window is hidden; normal native presentation cannot be qualified".into());}
            report.push_str("window_visible=true\n");
            #[cfg(feature="retained-wgpu")]
            report.push_str(&view.read_with(cx,|this,_|format!("adapter={:?}\n",this.gpu.as_ref().map(|gpu|gpu.adapter_info()))));
            // The debug CPU raster can exceed the short intervals. Keep those
            // observations and separately qualify forward progress over 2s.
            let stalls:&[u64]=if cpu {&[100,500,2000]} else {&[100,500]};
            for (phase,millis) in stalls.iter().copied().enumerate() {
                // Use actual GPUI event dispatch at installed geometry.
                AnyWindowHandle::from(window).update(cx,|_,w,cx| {
                    let point={let this=view.read(cx);let shown=this.shown.as_ref().unwrap();let item=shown.scene.frame().resolve_geometry(this.animated.unwrap()).unwrap();this.bounds.unwrap().origin+point(px(item.bounds.x+30.0),px(item.bounds.y+24.0))};
                    w.dispatch_event(MouseDownEvent {position:point,button:MouseButton::Left,..Default::default()}.to_platform_input(),cx);
                    w.dispatch_event(MouseUpEvent {position:point,button:MouseButton::Left,..Default::default()}.to_platform_input(),cx);
                }).map_err(|e|e.to_string())?;
                for _ in 0..300 {
                    cx.background_executor().timer(Duration::from_millis(5)).await;
                    if view.read_with(cx,|this,_|this.count==phase as u32+1&&this.timeline.as_ref().is_some_and(AnimationRuntime::is_animating)&&this.outstanding==0) {break;}
                }
                let (probe,old_turn,start_index,started,sent_at)=view.update(cx,|this,cx| {
                    if let Some(e)=&this.error {return Err(e.clone());}
                    if this.count!=phase as u32+1 {return Err(format!("click not processed: {}",this.count));}
                    let index=this.installed_during_stall.len();let turn=this.owner_turn;let started=this.probe.stall_started.load(Ordering::SeqCst);
                    let sent_at=Instant::now();
                    if !this.send(OwnerCommand::Stall {millis},cx) {return Err("stall request rejected".into());}
                    Ok((this.probe.clone(),turn,index,started,sent_at))
                }).map_err(|e|e.to_string())?;
                let mut attempts=BTreeSet::new();let mut records=vec![];let mut active=false;
                // No window.draw/present_if_needed here. The normal platform
                // callbacks, animation demand and CPU completion drive output.
                for _ in 0..500 {
                    cx.background_executor().timer(Duration::from_millis(5)).await;
                    let stalled=probe.stalled.load(Ordering::SeqCst);
                    if stalled {
                        active=true;
                        let (turn,attempt)=AnyWindowHandle::from(window).update(cx,|_,w,cx| {let this=view.read(cx);(this.owner_turn,w.presentation_attempt())}).map_err(|e|e.to_string())?;
                        if turn!=old_turn {return Err("UI owner adopted a turn inside its stall".into());}
                        if let Some(a)=attempt && a.queued && a.started_at>=sent_at && attempts.insert((a.device_epoch,a.sequence)) {records.push(format!("stall_ms={millis},present={},queued={},status={},started={:?},returned={:?}",a.sequence,a.queued,a.status,a.started_at,a.returned_at));}
                    }
                    if probe.stall_finished.load(Ordering::SeqCst)>started {break;}
                }
                if !active {return Err("owner stall was not observed live".into());}
                let installed=view.read_with(cx,|this,_|this.installed_during_stall[start_index..].to_vec());
                let revisions=installed.iter().map(|(_,r,_)|*r).collect::<BTreeSet<_>>();
                let translations=installed.iter().map(|(_,_,p)|p[0].to_bits()).collect::<BTreeSet<_>>();
                let require_progress=!cpu||millis==2000;
                if require_progress && (revisions.len()<2||translations.len()<2) {return Err(format!("no advancing complete samples during {millis}ms: {} revisions / {} placements",revisions.len(),translations.len()));}
                if require_progress && attempts.len()<2 {return Err(format!("no advancing normal Present attempts during {millis}ms: {}",attempts.len()));}
                report.push_str(&format!("stall_ms={millis},owner_turn={old_turn},installed_samples={},translations={},queued_present_attempts={}\n",revisions.len(),translations.len(),attempts.len()));
                std::fs::write(output.join(format!("present-{millis}.txt")),records.join("\n")).map_err(|e|e.to_string())?;
                // Allow queued input/adoption feedback to settle and the spring
                // to reach its exact terminal frame, without blocking the host.
                for _ in 0..500 {
                    cx.background_executor().timer(Duration::from_millis(10)).await;
                    if view.read_with(cx,|this,_|this.outstanding==0&&this.cpu_task.is_none()&&this.ready.is_none()&&!this.timeline.as_ref().is_some_and(AnimationRuntime::is_animating)) {break;}
                }
                let pixels=AnyWindowHandle::from(window).update(cx,|_,w,cx| {
                    w.draw(cx).clear(cx);w.present_if_needed();
                    let this=view.read(cx);if let Some(e)=&this.error {return Err(e.clone());}
                    let shown=this.shown.as_ref().ok_or("no installed image")?;
                    if this.router.scene_id()!=Some(shown.scene.id()) {return Err("input/image mismatch".into());}
                    let mut job=SoftwareJob::new(shown.scene.frame().resolve(),SoftwareOptions {size:shown.size,scale:shown.scale,background:0,effects:pacing::SoftwareEffects::Exact,max_bytes:128*1024*1024}).map_err(|e|format!("{e:?}"))?;
                    while !job.step(8192).map_err(|e|format!("{e:?}"))? {}let reference=job.into_frame().map_err(|e|format!("{e:?}"))?;
                    let image=w.render_to_image().map_err(|e|e.to_string())?;let b=this.bounds.unwrap();let origin=[f32::from(b.origin.x),f32::from(b.origin.y)].map(|v|(v*shown.scale).round()as u32);
                    for y in 0..shown.size[1] {for x in 0..shown.size[0] {let actual=image.get_pixel(origin[0]+x,origin[1]+y).0;let expected=&reference.pixels()[((y*shown.size[0]+x)*4)as usize..][..4];if actual.iter().zip(expected).any(|(a,b)|a.abs_diff(*b)>1) {return Err(format!("native pixel differs {phase} {x},{y}: {actual:?}/{expected:?}"));}}}
                    image.save(output.join(format!("stage-{phase}.png"))).map_err(|e|e.to_string())?;
                    #[cfg(all(feature="retained-wgpu",target_os="windows"))]
                    report.push_str(&format!("phase={phase},native_surface={}\n",matches!(shown.raster,Raster::Gpu(_))));
                    Ok(shown.size[0]as u64*shown.size[1]as u64)
                }).map_err(|e|e.to_string())??;
                report.push_str(&format!("phase={phase},native_pixels={pixels}\n"));
            }
            // Keyboard activation/reveal remains on the UI owner after stalls.
            AnyWindowHandle::from(window).update(cx,|_,w,cx| {w.dispatch_keystroke(Keystroke::parse("tab").unwrap(),cx);w.dispatch_keystroke(Keystroke::parse("space").unwrap(),cx);w.dispatch_event(KeyUpEvent {keystroke:Keystroke::parse("space").unwrap()}.to_platform_input(),cx);}).map_err(|e|e.to_string())?;
            let expected_count=stalls.len()as u32+1;
            for _ in 0..400 {cx.background_executor().timer(Duration::from_millis(10)).await;if view.read_with(cx,|this,_|this.count==expected_count&&this.outstanding==0) {break;}}
            view.read_with(cx, |this, _| {
                if let Some(e) = &this.error { return Err(e.clone()); }
                if this.count != expected_count { return Err(format!("keyboard count {} !={expected_count}", this.count)); }
                if this.router.captured().is_some() { return Err("capture not released".into()); }
                Ok(())
            })?;
            report.push_str(&format!("keyboard_count={expected_count}\ncapture_released=true\n"));
            Ok(report)
        }.await;
        let text=match result {Ok(report)=>format!("passed=true\nmode={}\nUI=dedicated LogicalUi/Flex owner thread\npresentation=normal GPUI callbacks while owner stalled\nevidence=native Present acceptance, not compositor/display completion\ninput=app-internal GPUI dispatch, not OS injection\n{report}",if cpu{"CPU"}else{"GPU"}),Err(error)=>format!("passed=false\n{error}\n")};
        std::fs::write(output.join("result.txt"),text).unwrap();cx.update(|cx|cx.quit());
    }).detach();
}
