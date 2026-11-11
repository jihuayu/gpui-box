use gpui::{AppContext, TestAppContext};
use gpui_box_components::reactive::{Binding, ReactiveScope, Store};

#[derive(Clone, PartialEq)]
struct Preferences {
    dark: bool,
    count: usize,
}
struct Screen {
    scope: ReactiveScope,
}

#[gpui::test]
fn entity_scopes_batch_selectors_bindings_and_cleanup(cx: &mut TestAppContext) {
    let (store, dark, count) = cx.update(|cx| {
        let store = Store::new(
            Preferences {
                dark: false,
                count: 0,
            },
            cx,
        );
        let dark = store.select(|state| state.dark, cx);
        let count = store.select(|state| state.count, cx);
        (store, dark, count)
    });
    let screen = cx.new(|cx| {
        let mut scope = ReactiveScope::default();
        scope.observe(&dark.signal(), cx);
        scope.observe(&count.signal(), cx);
        Screen { scope }
    });
    cx.update(|cx| {
        store.command(
            |state| {
                state.dark = true;
                state.count = 1;
            },
            cx,
        );
        store.command(|state| state.count = 2, cx);
    });
    cx.read(|cx| {
        assert!(dark.get(cx));
        assert_eq!(count.get(cx), 2);
        assert_eq!(screen.read(cx).scope.notification_count(), 1);
    });
    cx.update(|cx| {
        assert!(!store.command(|_| {}, cx));
    });
    cx.read(|cx| assert_eq!(screen.read(cx).scope.notification_count(), 1));
    screen.update(cx, |screen, _| screen.scope.clear());
    cx.update(|cx| {
        let binding = Binding::signal(store.signal());
        binding.set(
            Preferences {
                dark: false,
                count: 8,
            },
            cx,
        );
        assert_eq!(binding.get(cx).count, 8);
    });
    cx.read(|cx| {
        assert_eq!(screen.read(cx).scope.notification_count(), 1);
        assert_eq!(count.get(cx), 8);
    });
    drop(screen);
    drop(dark);
    drop(count);
    cx.update(|cx| {
        store.command(|state| state.count = 9, cx);
    });
}

#[gpui::test]
fn selectors_skip_unrelated_commands_and_drop_releases_sources(cx: &mut TestAppContext) {
    let (store, selected) = cx.update(|cx| {
        let store = Store::new(
            Preferences {
                dark: false,
                count: 0,
            },
            cx,
        );
        let selected = store.select(|state| state.dark, cx);
        (store, selected)
    });
    let screen = cx.new(|cx| {
        let mut scope = ReactiveScope::default();
        scope.observe(&selected.signal(), cx);
        Screen { scope }
    });
    cx.update(|cx| {
        store.command(|state| state.count += 1, cx);
    });
    cx.read(|cx| assert_eq!(screen.read(cx).scope.notification_count(), 0));
    cx.update(|cx| {
        store.command(|state| state.dark = true, cx);
    });
    cx.read(|cx| assert_eq!(screen.read(cx).scope.notification_count(), 1));
    drop(store);
    drop(selected);
    drop(screen);
}
