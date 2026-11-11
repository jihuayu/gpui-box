//! Opt-in real desktop executor/surface check. Timers only drive this diagnostic;
//! normal source delivery itself has no timer or idle polling loop.
use super::*;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

pub(crate) fn open(cx: &mut App, output: PathBuf) {
    let window = cx
        .open_window(WindowOptions::default(), |_, cx| {
            cx.new(RetainedDemo::with_gpu)
        })
        .expect("open source-delivery desktop check");
    let view = window
        .update(cx, |_, _, cx| cx.entity())
        .expect("get diagnostic view");
    cx.activate(true);
    cx.spawn(async move |cx| {
        let mut report = String::new();
        let result: Result<(),String> = async {
            cx.background_executor().timer(Duration::from_millis(350)).await;
            let (sender, before, first_renders) = view.read_with(cx, |view, _| (
                view.color_sender.clone(), view.tree.borrow().stats(), view.render_count,
            ));
            cx.background_executor().timer(Duration::from_millis(250)).await;
            let idle_renders = view.read_with(cx, |view,_| view.render_count);
            let native_state = cx.update(|cx| {
                let environment = cx.graphical_environment().is_some();
                window.update(cx, |_, window, _| (environment, window.visibility())).expect("native state")
            });
            if idle_renders != first_renders { return Err("initial idle view rendered again".into()); }
            // Join the short producer while the owner is occupied, making the
            // burst boundary exact. The worker still wakes the real foreground
            // executor from an actual OS thread.
            let start = Instant::now();
            let worker = sender.clone();
            std::thread::spawn(move || {
                for _ in 0..500 {
                    worker.send(0x4488ffff).unwrap();
                    worker.send(0x66aa88ff).unwrap();
                }
            }).join().map_err(|_| "producer panicked")?;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            let automatic_renders = view.read_with(cx, |view, _| view.render_count-idle_renders);
            let manual_hidden_draw = !native_state.1.is_visible();
            if manual_hidden_draw {
                // Hidden windows intentionally do not consume native presents.
                // Draw explicitly only for this diagnostic and label that fact.
                cx.update(|cx| AnyWindowHandle::from(window).update(cx, |_, window, cx| window.draw(cx).clear(cx)).expect("draw hidden diagnostic"));
            }
            let (after, renders, revision, pixel_equal, source_final, queue) = view.update(cx, |view, _| {
                let tree = view.tree.borrow();
                let packet = tree.published_frame().unwrap();
                let gpu = view.gpu.as_ref().unwrap();
                let mut gpu = gpu.borrow_mut();
                let actual = gpu.image().unwrap();
                // Native scale is reflected by the actual surface dimensions;
                // use the packet's viewport width resolved by root layout.
                let root = &packet.items[0];
                let scale = actual.width() as f32 / root.bounds.width;
                let reference = gpu.reference_image(packet, [actual.width(),actual.height()],scale).unwrap();
                (tree.stats(),view.render_count,packet.revision,actual.as_raw()==reference.as_raw(),view.observed_color.get(),sender.stats().unwrap())
            });
            let worker_to_checked_us = start.elapsed().as_micros();
            if after.transactions != before.transactions+1 || after.computed_evaluations != before.computed_evaluations+1 || after.effect_runs != before.effect_runs+1 {
                return Err("burst was not one computed/effect transaction".into());
            }
            let expected_automatic = usize::from(!manual_hidden_draw);
            if automatic_renders != expected_automatic || renders != idle_renders+1 || source_final != 0x66aa88ff || !pixel_equal { return Err(format!("native view/material/pixels differ: automatic={automatic_renders}, renders={renders}, idle={idle_renders}, final={source_final:08x}, pixel_equal={pixel_equal}, native={native_state:?}")); }
            if queue.coalesced!=999 || queue.pending!=0 || queue.waker_notifications!=1 { return Err("burst queue/wake counts differ".into()); }
            cx.background_executor().timer(Duration::from_millis(250)).await;
            if view.read_with(cx, |view,_| view.render_count)!=renders { return Err("post-update idle view rendered again".into()); }
            let equal = sender.clone();
            std::thread::spawn(move || equal.send(0x66aa88ff).unwrap()).join().map_err(|_| "equal producer panicked")?;
            cx.background_executor().timer(Duration::from_millis(150)).await;
            let (final_renders, final_revision, final_queue) = view.read_with(cx, |view,_| (view.render_count,view.tree.borrow().published_frame().unwrap().revision,sender.stats().unwrap()));
            if final_renders!=renders || final_revision!=revision || final_queue.waker_notifications!=2 { return Err("equal input changed visual output or did not wake owner".into()); }
            // Submit two independent offscreen diagnostic packets, then let
            // only the native completion task retire them. No image/finish/
            // submit/poll call on the owner occurs between submit and check.
            let (completion_serial, weak_frame, weak_pixels) = view.update(cx, |view, _| {
                let mut tree=UiTree::new(Style::default(),0x112233ff).unwrap();
                let image=ImageResource::rgba8([2,2],vec![255;16]).unwrap();
                let weak_pixels=std::sync::Arc::downgrade(image.pixels());
                let mut tx=Transaction::default();
                tx.image(tree.root(),Some(image));
                tree.commit(tx,BackendCapabilities::default()).unwrap();
                let first=tree.frame([32.0,32.0]).unwrap();
                let weak_frame=std::sync::Arc::downgrade(&first.items);
                let gpu=view.gpu.as_ref().unwrap();
                gpu.borrow_mut().submit(&first,[32,32],1.0).unwrap();
                let mut tx=Transaction::default();
                tx.image(tree.root(),None);
                tree.commit(tx,BackendCapabilities::default()).unwrap();
                let next=tree.frame([32.0,32.0]).unwrap();
                let serial=gpu.borrow_mut().submit(&next,[32,32],1.0).unwrap().unwrap().gpu.serial;
                (serial,weak_frame,weak_pixels)
            });
            cx.background_executor().timer(Duration::from_millis(250)).await;
            let (lifecycle, completion_stats, completion_renders) = view.read_with(cx, |view,_| {
                let gpu=view.gpu.as_ref().unwrap().borrow();
                (gpu.lifecycle(),gpu.completion_stats().unwrap(),view.render_count)
            });
            if lifecycle.completed<completion_serial || lifecycle.frame_leases!=0 || lifecycle.gpu_leases!=0 || weak_frame.upgrade().is_some() || weak_pixels.upgrade().is_some() {
                return Err(format!("native idle completion did not retire: {lifecycle:?}"));
            }
            if completion_renders!=renders { return Err("completion collection redrew the view".into()); }
            cx.background_executor().timer(Duration::from_millis(250)).await;
            let idle_completion_stats=view.read_with(cx,|view,_| view.gpu.as_ref().unwrap().borrow().completion_stats().unwrap());
            if completion_stats!=idle_completion_stats {return Err("idle completion worker performed extra work".into());}
            report = format!("{{\"passed\":true,\"platform\":\"{}\",\"real_os_worker\":true,\"real_foreground_executor\":true,\"native_window\":true,\"native_window_visible\":{},\"manual_hidden_draw\":{manual_hidden_draw},\"automatic_view_renders\":{automatic_renders},\"source_messages\":1000,\"coalesced\":999,\"burst_waker_notifications\":1,\"burst_transactions\":1,\"burst_computed_evaluations\":1,\"burst_effect_runs\":1,\"burst_view_renders\":1,\"pixel_equal\":true,\"equal_additional_view_renders\":0,\"initial_idle_ms\":250,\"post_update_idle_ms\":250,\"completion_retired_without_new_frame\":true,\"completion_frame_leases\":0,\"completion_gpu_leases\":0,\"completion_extra_view_renders\":0,\"completion_idle_counters_stable_ms\":250,\"worker_to_delayed_check_us\":{worker_to_checked_us},\"timing_note\":\"Includes deliberate 150ms diagnostic wait; not wake or input latency. Hidden window draw is explicit, not automatic presentation. Idle checks count view renders and driver work, not OS process wakeups or energy.\"}}",std::env::consts::OS,native_state.1.is_visible());
            Ok(())
        }.await;
        if let Err(error)=result {
            eprintln!("Native source delivery check failed: {error}");
            report = format!("{{\"passed\":false,\"error\":{:?}}}",error);
        }
        if let Some(parent)=output.parent() { let _=std::fs::create_dir_all(parent); }
        std::fs::write(&output,report).expect("write desktop source delivery result");
        cx.update(|cx| cx.quit());
    }).detach();
}
