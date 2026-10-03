use super::*;
use crate::tests::open;
use crate::workspace::Workspace;
use ::core::prelude::v1::test;
use emulsion_core::creation::{CanvasKind, CanvasSpec};
use gpui_kit::test::TestWindowExt;

fn select_all() -> &'static str {
    if cfg!(target_os = "macos") {
        "cmd-a"
    } else {
        "ctrl-a"
    }
}

/// A two-panel storyboard in its workspace, with the inspector showing.
fn storyboard(
    cx: &mut TestAppContext,
) -> (
    Entity<Workspace>,
    Entity<EditorView>,
    &mut VisualTestContext,
) {
    let (ws, cx) = open(cx, Document::new(64, 36));
    cx.simulate_resize(size(px(1600.), px(2400.)));
    // Focus and blur events need an active window.
    cx.update(|window, _| window.activate_window());
    let project = CanvasSpec {
        name: "Board".into(),
        kind: CanvasKind::Storyboard,
        width: 64.,
        height: 36.,
        pages: 2,
        ..Default::default()
    }
    .create_project()
    .unwrap();
    let view = cx.update(|window, cx| {
        ws.update(cx, |ws, cx| {
            ws.install_project(project, "Board".into(), window, cx)
        });
        ws.read(cx).editor.clone().unwrap()
    });
    settle(cx);
    cx.update(|window, _| assert!(window.find("storyboard-inspector").visible()));
    (ws, view, cx)
}

fn settle(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.render_frame(cx));
    cx.run_until_parked();
}

fn focus(
    cx: &mut VisualTestContext,
    view: &Entity<EditorView>,
    pick: impl Fn(&Inspector) -> AnyInput,
) {
    cx.update(
        |window, cx| match pick(view.read(cx).storyboard_ui.inspector.as_ref().unwrap()) {
            AnyInput::Line(state) => state.update(cx, |s, cx| s.focus(window, cx)),
            AnyInput::Area(state) => state.update(cx, |s, cx| s.focus(window, cx)),
        },
    );
    settle(cx);
}

/// Replace the focused input's text.
fn type_text(cx: &mut VisualTestContext, text: &str) {
    cx.simulate_keystrokes(select_all());
    cx.simulate_input(text);
    settle(cx);
}

/// Move focus to the canvas, so the focused field loses it.
fn blur(cx: &mut VisualTestContext, view: &Entity<EditorView>) {
    cx.update(|window, cx| {
        let focus = view.read(cx).canvas_focus.clone();
        window.focus(&focus, cx);
    });
    settle(cx);
}

enum AnyInput {
    Line(Entity<InputState>),
    Area(Entity<TextareaState>),
}

fn caption_input(inspector: &Inspector, field: CaptionId) -> AnyInput {
    match &inspector
        .captions
        .iter()
        .find(|(id, _)| *id == field)
        .unwrap()
        .1
    {
        CaptionInput::Line(state) => AnyInput::Line(state.clone()),
        CaptionInput::Area(state) => AnyInput::Area(state.clone()),
    }
}

fn board(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Storyboard {
    cx.update(|_, cx| view.read(cx).editor.storyboard().unwrap().clone())
}

fn active(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> PageId {
    cx.update(|_, cx| view.read(cx).editor.active_page())
}

fn undo(view: &Entity<EditorView>, cx: &mut VisualTestContext) {
    cx.update(|_, cx| view.update(cx, |v, cx| v.undo(cx)));
    settle(cx);
}

#[gpui_kit::test]
fn storyboard_inspector_edits_commit_once_as_single_undo_steps(cx: &mut TestAppContext) {
    let (_ws, view, cx) = storyboard(cx);
    let panel = active(&view, cx);
    assert_eq!(board(&view, cx).panels[&panel].frames, 48);

    focus(cx, &view, |i| AnyInput::Line(i.frames.clone()));
    type_text(cx, "72");
    cx.simulate_keystrokes("enter");
    settle(cx);
    assert_eq!(board(&view, cx).panels[&panel].frames, 72);
    // Seconds convert at the project frame rate (24 fps).
    focus(cx, &view, |i| AnyInput::Line(i.seconds.clone()));
    type_text(cx, "1.5");
    cx.simulate_keystrokes("enter");
    settle(cx);
    assert_eq!(board(&view, cx).panels[&panel].frames, 36);
    undo(&view, cx);
    assert_eq!(board(&view, cx).panels[&panel].frames, 72);
    undo(&view, cx);
    assert_eq!(board(&view, cx).panels[&panel].frames, 48);

    // Typing a caption changes nothing until the field loses focus.
    let action = board(&view, cx).caption("Action").unwrap();
    focus(cx, &view, |i| caption_input(i, action));
    cx.simulate_input("Hero runs");
    settle(cx);
    cx.simulate_input(" fast");
    settle(cx);
    assert!(
        !board(&view, cx).panels[&panel]
            .captions
            .contains_key(&action)
    );
    blur(cx, &view);
    assert_eq!(
        board(&view, cx).panels[&panel].captions[&action].text,
        "Hero runs fast"
    );
    undo(&view, cx);
    assert!(
        !board(&view, cx).panels[&panel]
            .captions
            .contains_key(&action)
    );

    // Shot data and tags through their controls.
    cx.update(|window, cx| window.click(("storyboard-tag", 2usize), cx));
    settle(cx);
    assert_eq!(board(&view, cx).panels[&panel].tag, Some(2));
    undo(&view, cx);
    assert_eq!(board(&view, cx).panels[&panel].tag, None);

    // The name commits through the page list.
    focus(cx, &view, |i| AnyInput::Line(i.name.clone()));
    type_text(cx, "Opening");
    cx.simulate_keystrokes("enter");
    settle(cx);
    cx.update(|_, cx| {
        let editor = &view.read(cx).editor;
        assert_eq!(
            editor
                .page_list()
                .iter()
                .find(|m| m.id == panel)
                .unwrap()
                .name,
            "Opening"
        );
    });

    // Another panel rebinds the inputs to it.
    let other = cx.update(|_, cx| view.read(cx).editor.page_list()[1].id);
    cx.update(|_, cx| view.update(cx, |v, cx| v.select_page(other, cx)));
    settle(cx);
    cx.update(|_, cx| {
        let inspector = view.read(cx).storyboard_ui.inspector.as_ref().unwrap();
        assert_eq!(inspector.panel, other);
        assert_eq!(inspector.frames.read(cx).value(), "48");
    });
}

#[gpui_kit::test]
fn storyboard_locked_panels_are_read_only_until_unlocked(cx: &mut TestAppContext) {
    let (_ws, view, cx) = storyboard(cx);
    let panel = active(&view, cx);
    cx.update(|window, cx| window.click("storyboard-lock-panel", cx));
    settle(cx);
    assert!(board(&view, cx).panels[&panel].locked);
    cx.update(|window, _| assert!(window.find("storyboard-locked").visible()));

    // Read-only inputs take no typing, and direct edits are refused.
    focus(cx, &view, |i| AnyInput::Line(i.frames.clone()));
    type_text(cx, "99");
    cx.simulate_keystrokes("enter");
    settle(cx);
    assert_eq!(board(&view, cx).panels[&panel].frames, 48);
    cx.update(|_, cx| view.update(cx, |v, cx| v.edit_panel(panel, |p| p.frames = 99, cx)));
    settle(cx);
    assert_eq!(board(&view, cx).panels[&panel].frames, 48);
    cx.update(|window, _| {
        let error = window.find("storyboard-error");
        assert!(error.label().unwrap().contains("locked"));
    });

    // A locked scene locks the panel too; Unlock clears both.
    cx.update(|window, cx| window.click("storyboard-lock-scene", cx));
    settle(cx);
    let scene = board(&view, cx).panels[&panel].scene;
    assert!(board(&view, cx).scenes[&scene].locked);
    cx.update(|window, cx| window.click("storyboard-unlock", cx));
    settle(cx);
    let b = board(&view, cx);
    assert!(!b.is_locked(panel) && !b.scenes[&scene].locked);
    cx.update(|window, _| assert!(window.try_find("storyboard-locked").is_none()));
    focus(cx, &view, |i| AnyInput::Line(i.frames.clone()));
    type_text(cx, "60");
    cx.simulate_keystrokes("enter");
    settle(cx);
    assert_eq!(board(&view, cx).panels[&panel].frames, 60);
}

#[gpui_kit::test]
fn storyboard_caption_formatting_applies_to_the_selection(cx: &mut TestAppContext) {
    let (_ws, view, cx) = storyboard(cx);
    let panel = active(&view, cx);
    let dialogue = board(&view, cx).caption("Dialogue").unwrap();
    focus(cx, &view, |i| caption_input(i, dialogue));
    cx.simulate_input("Hero runs");
    settle(cx);
    // Select "runs", then format it.
    for _ in 0..4 {
        cx.simulate_keystrokes("shift-left");
    }
    settle(cx);
    cx.update(|window, cx| window.click("storyboard-format-bold", cx));
    settle(cx);
    let caption = board(&view, cx).panels[&panel].captions[&dialogue].clone();
    assert_eq!(caption.text, "Hero runs");
    assert!(caption.style_at(5).bold && !caption.style_at(0).bold);
    cx.update(|window, _| {
        window.find(("storyboard-caption-preview", dialogue as usize));
    });
    // Toggling again clears it, as one more Undo step.
    cx.update(|window, cx| window.click("storyboard-format-bold", cx));
    settle(cx);
    assert!(
        !board(&view, cx).panels[&panel].captions[&dialogue]
            .style_at(5)
            .bold
    );
    undo(&view, cx);
    assert!(
        board(&view, cx).panels[&panel].captions[&dialogue]
            .style_at(5)
            .bold
    );

    // Underline, strikethrough and italic on the same selection.
    for id in [
        "storyboard-format-italic",
        "storyboard-format-underline",
        "storyboard-format-strikethrough",
    ] {
        cx.update(|window, cx| window.click(id, cx));
        settle(cx);
    }
    let style = board(&view, cx).panels[&panel].captions[&dialogue].style_at(6);
    assert!(style.italic && style.underline && style.strikethrough);

    // Colour through the app's colour picker.
    cx.update(|window, cx| window.click("storyboard-format-color", cx));
    settle(cx);
    cx.update(|window, cx| window.click("style-color-swatch-3", cx));
    settle(cx);
    cx.update(|window, cx| window.click("caption-color-ok", cx));
    settle(cx);
    let caption = board(&view, cx).panels[&panel].captions[&dialogue].clone();
    assert_ne!(caption.style_at(6).color, Caption::base_style().color);
    assert_eq!(caption.style_at(0).color, Caption::base_style().color);
}

#[gpui_kit::test]
fn storyboard_caption_fields_add_rename_reorder_and_remove(cx: &mut TestAppContext) {
    let (_ws, view, cx) = storyboard(cx);
    cx.update(|window, cx| window.click("storyboard-fields-toggle", cx));
    settle(cx);
    focus(cx, &view, |i| AnyInput::Line(i.new_field.clone()));
    cx.simulate_input("Camera");
    cx.simulate_keystrokes("enter");
    settle(cx);
    let camera = board(&view, cx).caption("Camera").expect("field added");
    assert_eq!(board(&view, cx).captions.last().unwrap().id, camera);

    let rename = |i: &Inspector| {
        AnyInput::Line(
            i.field_names
                .iter()
                .find(|(id, _)| *id == camera)
                .unwrap()
                .1
                .clone(),
        )
    };
    focus(cx, &view, rename);
    type_text(cx, "Lens");
    cx.simulate_keystrokes("enter");
    settle(cx);
    assert_eq!(board(&view, cx).caption("Lens"), Some(camera));
    // Names stay unique.
    focus(cx, &view, rename);
    type_text(cx, "action");
    cx.simulate_keystrokes("enter");
    settle(cx);
    assert_eq!(board(&view, cx).caption("Lens"), Some(camera));
    cx.update(|window, _| {
        assert!(
            window
                .find("storyboard-error")
                .label()
                .unwrap()
                .contains("unique")
        )
    });

    let key = camera as usize;
    cx.update(|window, cx| window.click(("storyboard-field-multiline", key), cx));
    settle(cx);
    cx.update(|window, cx| window.click(("storyboard-field-print", key), cx));
    settle(cx);
    let field = board(&view, cx)
        .captions
        .iter()
        .find(|c| c.id == camera)
        .cloned()
        .unwrap();
    assert!(!field.multiline && !field.print);
    cx.update(|window, cx| window.click(("storyboard-field-up", key), cx));
    settle(cx);
    let order: Vec<_> = board(&view, cx).captions.iter().map(|c| c.id).collect();
    assert_eq!(order[order.len() - 2], camera);
    // The caption editor follows the field order.
    cx.update(|_, cx| {
        let inspector = view.read(cx).storyboard_ui.inspector.as_ref().unwrap();
        let ids: Vec<_> = inspector.captions.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids, order);
    });

    cx.update(|window, cx| window.click(("storyboard-field-remove", key), cx));
    settle(cx);
    assert!(board(&view, cx).caption("Lens").is_none());
    undo(&view, cx);
    assert_eq!(board(&view, cx).caption("Lens"), Some(camera));
}

#[gpui_kit::test]
fn storyboard_find_and_replace_counts_skips_locked_panels_and_undoes(cx: &mut TestAppContext) {
    let (_ws, view, cx) = storyboard(cx);
    let pages: Vec<_> = cx.update(|_, cx| {
        view.read(cx)
            .editor
            .page_list()
            .iter()
            .map(|m| m.id)
            .collect()
    });
    let b = board(&view, cx);
    let (action, dialogue) = (b.caption("Action").unwrap(), b.caption("Dialogue").unwrap());
    cx.update(|_, cx| {
        view.update(cx, |v, cx| {
            assert!(v.edit_board(
                |b| {
                    let first = b.panels.get_mut(&pages[0]).unwrap();
                    first.captions.insert(action, "Mia runs. mia waves.".into());
                    first.captions.insert(dialogue, "Mian?".into());
                    let second = b.panels.get_mut(&pages[1]).unwrap();
                    second.captions.insert(dialogue, "Mia!".into());
                    second.locked = true;
                    Ok(())
                },
                cx,
            ));
        })
    });
    settle(cx);
    let before = board(&view, cx);
    cx.update(|window, cx| {
        window.dispatch_action(Box::new(crate::actions::FindReplaceCaptions), cx)
    });
    settle(cx);
    cx.update(|window, _| assert!(window.find("caption-find").visible()));
    // The query field has focus when the dialog opens.
    cx.simulate_input("mia");
    settle(cx);
    cx.update(|window, _| {
        for index in 0usize..4 {
            window.find(("caption-find-result", index));
        }
        assert!(window.try_find(("caption-find-result", 4usize)).is_none());
    });
    cx.update(|window, cx| window.click("caption-find-word", cx));
    settle(cx);
    cx.update(|window, _| {
        assert!(window.try_find(("caption-find-result", 3usize)).is_none());
    });
    // Clicking a result opens its panel.
    cx.update(|window, cx| window.click(("caption-find-result", 2usize), cx));
    settle(cx);
    assert_eq!(active(&view, cx), pages[1]);

    cx.update(|window, cx| window.click("caption-find-replacement", cx));
    settle(cx);
    cx.simulate_input("Ana");
    settle(cx);
    cx.update(|window, cx| window.click("caption-find-replace-all", cx));
    settle(cx);
    cx.update(|window, _| {
        assert_eq!(
            window.find("caption-find-message").label(),
            Some("Replaced 2 matches. Skipped 1 locked panel.")
        );
    });
    let after = board(&view, cx);
    assert_eq!(
        after.panels[&pages[0]].captions[&action].text,
        "Ana runs. Ana waves."
    );
    assert_eq!(after.panels[&pages[0]].captions[&dialogue].text, "Mian?");
    assert_eq!(after.panels[&pages[1]].captions[&dialogue].text, "Mia!");
    // Replace All is one Undo step.
    undo(&view, cx);
    assert_eq!(board(&view, cx), before);
}

#[gpui_kit::test]
fn storyboard_caption_text_keeps_formatting_around_edits(_cx: &mut TestAppContext) {
    let mut caption = Caption::from("Mia waves at Tom");
    caption.apply_style(0..3, |s| s.bold = true);
    caption.apply_style(13..16, |s| s.italic = true);
    set_caption_text(&mut caption, "Mia smiles at Tom");
    assert_eq!(caption.text, "Mia smiles at Tom");
    assert!(caption.style_at(0).bold && caption.style_at(15).italic);
    assert!(!caption.style_at(5).bold && !caption.style_at(5).italic);
    set_caption_text(&mut caption, "");
    assert!(caption.text.is_empty());
    caption.validate().unwrap();
}
