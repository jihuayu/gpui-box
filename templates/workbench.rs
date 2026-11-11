mod contracts;
mod documents;
mod providers;
mod search;

use contracts::{DocumentSummary, Documents};
use documents::DocumentModule;
use gpui::{prelude::FluentBuilder, *};
use gpui_box::menus::{
    Completion, FakeMenuProvider, InvocationContext, MenuCommandService, OperationContext,
    SurfaceId, UiOwner,
};
use gpui_box_components::{Layout, TextInput, Theme, app_bar, button, label, master_detail, shell};
use search::SearchModule;
use std::{future::Future, rc::Rc};

struct Workbench {
    commands: MenuCommandService,
    owner: UiOwner,
    documents: DocumentModule,
    search: SearchModule,
    query: Entity<TextInput>,
    editor: Entity<TextInput>,
    results: Vec<DocumentSummary>,
    selected: Option<DocumentSummary>,
    detail_open: bool,
    status: String,
    request_revision: u64,
    master_focus: FocusHandle,
}

impl Workbench {
    fn invocation(&self) -> InvocationContext {
        InvocationContext::new(OperationContext {
            ui: Some(self.owner.context(None).expect("live workbench owner")),
            ..OperationContext::default()
        })
    }

    fn run<T: std::fmt::Debug + 'static>(
        &mut self,
        request: impl Future<Output = gpui_box::menus::ServiceResult<T>> + 'static,
        apply: impl FnOnce(&mut Self, T, &mut Window, &mut Context<Self>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.request_revision += 1;
        let revision = self.request_revision;
        self.status = "Running command".into();
        let window = window.window_handle();
        cx.spawn(async move |this, cx| {
            let result = request.await;
            let _ = window.update(cx, |_, window, cx| {
                let _ = this.update(cx, |view, cx| {
                    if view.request_revision != revision {
                        return;
                    }
                    match result {
                        Ok(Completion::Completed(value)) => apply(view, value, window, cx),
                        other => view.status = format!("Command result · {other:?}",),
                    }
                    cx.notify();
                });
            });
        })
        .detach();
        cx.notify();
    }

    fn close_detail(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.detail_open = false;
        self.request_revision += 1;
        window.dismiss_virtual_keyboard();
        window.focus(&self.master_focus, cx);
        cx.notify();
    }
}

impl Drop for Workbench {
    fn drop(&mut self) {
        if let Ok(context) = self.owner.context(None) {
            self.commands.dispose_surface(context.surface);
        }
        self.owner.dispose();
    }
}

impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::DARK;
        let layout = Layout::for_width(f32::from(window.viewport_size().width));
        let compact_detail = layout == Layout::Compact && self.detail_open;
        let search = cx.entity().downgrade();
        let master = div()
            .flex()
            .flex_col()
            .gap_3()
            .child(label("search-title", "Search contribution", theme))
            .child(self.query.clone())
            .child(
                button("search", "Search documents", theme, move |window, cx| {
                    let _ = search.update(cx, |view, cx| {
                        let request = view.commands.invoke_typed(
                            &view.search.query,
                            view.query.read(cx).value().to_owned(),
                            view.invocation(),
                        );
                        window.blur(cx);
                        window.dismiss_virtual_keyboard();
                        view.run(
                            request,
                            |view, results, _, _| {
                                view.status = format!("Search results · {}", results.len());
                                view.results = results;
                            },
                            window,
                            cx,
                        );
                    });
                })
                .track_focus(&self.master_focus),
            )
            .children(self.results.iter().map(|(id, title)| {
                let entity = cx.entity().downgrade();
                let summary = (id.clone(), title.clone());
                button(
                    SharedString::from(format!("result-{id}")),
                    title.clone(),
                    theme,
                    move |window, cx| {
                        let _ = entity.update(cx, |view, cx| {
                            let request = view.commands.invoke_typed(
                                &view.documents.open,
                                summary.0.clone(),
                                view.invocation(),
                            );
                            let summary = summary.clone();
                            view.run(
                                request,
                                move |view, text, window, cx| {
                                    view.editor.update(cx, |editor, cx| {
                                        editor.set_value(text, window, cx)
                                    });
                                    view.selected = Some(summary);
                                    view.detail_open = true;
                                    view.status = "Document opened".into();
                                    window.blur(cx);
                                    window.dismiss_virtual_keyboard();
                                },
                                window,
                                cx,
                            );
                        });
                    },
                )
            }));
        let detail = self
            .selected
            .as_ref()
            .filter(|_| layout == Layout::Wide || self.detail_open)
            .map(|(id, title)| {
                let save = cx.entity().downgrade();
                let back = cx.entity().downgrade();
                let id = id.clone();
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .when(layout == Layout::Compact, |element| {
                        element.child(button(
                            "back",
                            "Back to search",
                            theme,
                            move |window, cx| {
                                let _ = back.update(cx, |view, cx| view.close_detail(window, cx));
                            },
                        ))
                    })
                    .child(label("document-title", title.clone(), theme))
                    .child(self.editor.clone())
                    .child(button("save", "Save document", theme, move |window, cx| {
                        let _ = save.update(cx, |view, cx| {
                            let request = view.commands.invoke_typed(
                                &view.documents.save,
                                (id.clone(), view.editor.read(cx).value().to_owned()),
                                view.invocation(),
                            );
                            window.blur(cx);
                            window.dismiss_virtual_keyboard();
                            view.run(
                                request,
                                |view, _, _, _| {
                                    view.status = "Saved to in-memory fake provider".into();
                                },
                                window,
                                cx,
                            );
                        });
                    }))
                    .into_any_element()
            });
        let content = master_detail(
            "workbench-panes",
            layout,
            master,
            detail,
            label(
                "empty-detail",
                "Choose a search result to open a document.",
                theme,
            ),
            theme,
        )
        .h_auto();
        let body = div()
            .id("workbench-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(content);
        shell::safe_area(window)
            .size_full()
            .flex()
            .flex_col()
            .tab_group()
            .font_family("Noto Sans CJK SC")
            .bg(rgb(theme.background))
            .text_color(rgb(theme.text))
            .on_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "tab" {
                    if event.keystroke.modifiers.shift {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                    cx.stop_propagation();
                }
                if compact_detail && event.keystroke.key == "escape" {
                    view.close_detail(window, cx);
                    cx.stop_propagation();
                }
            }))
            .when(compact_detail, |element| {
                element.on_back_gesture(cx.listener(
                    |view, event: &BackGestureEvent, window, cx| {
                        if event.phase == BackGesturePhase::Committed {
                            view.close_detail(window, cx);
                        }
                    },
                ))
            })
            .child(app_bar(
                "workbench-header",
                "Modular workbench",
                None,
                Vec::<AnyElement>::new(),
                theme,
            ))
            .child(
                label(
                    "provider-status",
                    "Providers · In-memory fake documents and command presentation",
                    theme,
                )
                .p_3(),
            )
            .child(
                label(
                    "layout-status",
                    format!("Layout · {layout:?} · {}", self.status),
                    theme,
                )
                .p_3(),
            )
            .child(
                shell::keyboard_avoidance(window, body)
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0(),
            )
    }
}

pub fn open(cx: &mut App) {
    cx.text_system()
        .add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!(
            "../../assets/NotoSansSC-Regular.otf"
        ))])
        .expect("load bundled font");
    gpui_box_components::input::bind_keys(cx);
    cx.open_window(WindowOptions::default(), |_, cx| {
        let query = cx.new(|cx| {
            let mut input = TextInput::new(cx);
            input.set_theme(Theme::DARK, cx);
            input.set_label("Document search query", cx);
            input
        });
        let editor = cx.new(|cx| {
            let mut input = TextInput::new(cx);
            input.set_theme(Theme::DARK, cx);
            input.set_label("Document editor", cx);
            input
        });
        cx.new(|cx| {
            let provider: Rc<dyn Documents> = Rc::new(providers::FakeDocuments::new());
            let commands = MenuCommandService::new(Rc::new(FakeMenuProvider::new()));
            let owner = UiOwner::new(SurfaceId(cx.entity_id().as_u64()));
            let documents = DocumentModule::activate(&commands, &owner, provider.clone())
                .expect("document contributions");
            let search = SearchModule::activate(&commands, &owner, provider.clone())
                .expect("search contributions");
            Workbench {
                commands,
                owner,
                documents,
                search,
                query,
                editor,
                results: provider.list(),
                selected: None,
                detail_open: false,
                status: "Ready".into(),
                request_revision: 0,
                master_focus: cx.focus_handle(),
            }
        })
    })
    .expect("open workbench");
    cx.activate(true);
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_box::menus::{CommandId, ErrorCode, ServiceResult};
    use std::task::{Context as TaskContext, Poll, Waker};

    fn completed<T>(request: impl Future<Output = ServiceResult<T>>) -> ServiceResult<T> {
        let mut request = std::pin::pin!(request);
        match request
            .as_mut()
            .poll(&mut TaskContext::from_waker(Waker::noop()))
        {
            Poll::Ready(result) => result,
            Poll::Pending => panic!("The in-memory UI command must complete synchronously"),
        }
    }

    fn invocation(owner: &UiOwner) -> InvocationContext {
        InvocationContext::new(OperationContext {
            ui: Some(owner.context(None).unwrap()),
            ..OperationContext::default()
        })
    }

    #[::core::prelude::v1::test]
    fn search_and_documents_share_only_the_injected_contract() {
        let provider: Rc<dyn Documents> = Rc::new(providers::FakeDocuments::new());
        let commands = MenuCommandService::new(Rc::new(FakeMenuProvider::new()));
        let owner = UiOwner::new(SurfaceId(42));
        let documents = DocumentModule::activate(&commands, &owner, provider.clone()).unwrap();
        let search = SearchModule::activate(&commands, &owner, provider.clone()).unwrap();
        let results =
            completed(commands.invoke_typed(&search.query, "WELCOME".into(), invocation(&owner)));
        assert_eq!(
            results.unwrap(),
            Completion::Completed(vec![("welcome".into(), "Welcome".into())])
        );
        assert_eq!(
            completed(commands.invoke_typed(&search.query, "unknown".into(), invocation(&owner)))
                .unwrap(),
            Completion::Completed(Vec::new())
        );
        assert_eq!(
            completed(commands.invoke_typed(
                &documents.save,
                ("welcome".into(), "Saved text".into()),
                invocation(&owner)
            ))
            .unwrap(),
            Completion::Completed("welcome".into())
        );
        assert_eq!(
            completed(commands.invoke_typed(&documents.open, "welcome".into(), invocation(&owner)))
                .unwrap(),
            Completion::Completed("Saved text".into())
        );
        drop(search);
        assert!(!commands.is_registered(&CommandId::new("search.query").unwrap()));
        assert!(commands.is_registered(&CommandId::new("documents.open").unwrap()));
        drop(documents);
        assert!(!commands.is_registered(&CommandId::new("documents.open").unwrap()));
        assert!(!commands.is_registered(&CommandId::new("documents.save").unwrap()));
    }

    #[::core::prelude::v1::test]
    fn released_owner_rejects_commands_before_provider_mutation() {
        let provider: Rc<dyn Documents> = Rc::new(providers::FakeDocuments::new());
        let commands = MenuCommandService::new(Rc::new(FakeMenuProvider::new()));
        let owner = UiOwner::new(SurfaceId(43));
        let documents = DocumentModule::activate(&commands, &owner, provider.clone()).unwrap();
        let context = invocation(&owner);
        let original = provider.read("welcome").unwrap();
        owner.dispose();
        let result = completed(commands.invoke_typed(
            &documents.save,
            ("welcome".into(), "Late write".into()),
            context,
        ));
        assert_eq!(result.unwrap_err().code, ErrorCode::ContextExpired);
        assert_eq!(provider.read("welcome").unwrap(), original);
    }
}
