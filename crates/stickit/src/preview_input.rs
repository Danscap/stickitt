//! Preview routing shares the editor's document, shortcuts and table syntax. No mutation here.
use crate::{config::NoteAction, editor::Doc};
use winit::event::{ElementState, Ime, WindowEvent};
use winit::keyboard::{Key, ModifiersState, NamedKey};

#[derive(Debug, PartialEq, Eq)]
pub enum Route {
    Edit,
    Read,
    Scroll,
    Ignore,
}

/// OS focus, close and modifier events are never evidence of an editor mutation.
pub fn event(doc: &Doc, event: &WindowEvent, mods: ModifiersState, action: Option<NoteAction>) -> Route {
    match event {
        WindowEvent::KeyboardInput { event, is_synthetic: false, .. } if event.state == ElementState::Pressed => {
            let primary =
                if cfg!(target_os = "macos") { mods.super_key() } else { mods.control_key() && !mods.alt_key() };
            key(doc, &event.logical_key, event.text.as_deref(), primary, mods.shift_key(), action)
        }
        WindowEvent::Ime(Ime::Commit(t) | Ime::Preedit(t, _)) if !t.is_empty() => Route::Edit,
        WindowEvent::Focused(_)
        | WindowEvent::MouseWheel { .. }
        | WindowEvent::CursorMoved { .. }
        | WindowEvent::MouseInput { .. } => Route::Read,
        _ => Route::Ignore,
    }
}

pub fn key(doc: &Doc, key: &Key, text: Option<&str>, primary: bool, shift: bool, action: Option<NoteAction>) -> Route {
    use Route::*;
    if primary && !shift && *key == Key::Named(NamedKey::Enter) && doc.can_subtask() {
        return Edit;
    }
    if let Some(action) = action {
        return match action {
            NoteAction::ToggleTodo
            | NoteAction::Bold
            | NoteAction::Italic
            | NoteAction::InlineCode
            | NoteAction::Strikethrough
            | NoteAction::Highlight => Edit,
            _ => Ignore,
        };
    }
    match key {
        Key::Named(NamedKey::ArrowUp | NamedKey::ArrowDown | NamedKey::PageUp | NamedKey::PageDown) => Scroll,
        Key::Named(NamedKey::ArrowLeft | NamedKey::ArrowRight | NamedKey::Home | NamedKey::End) => Read,
        Key::Character(c) if primary => match c.to_lowercase().as_str() {
            "a" | "c" => Read,
            "x" if doc.selection().is_some() => Edit,
            "v" | "z" | "y" => Edit,
            _ => Ignore,
        },
        Key::Named(named @ (NamedKey::Tab | NamedKey::Enter | NamedKey::Backspace | NamedKey::Delete)) => {
            if !primary {
                if let Some(g) = crate::table::grid_at(&doc.text, doc.cursor) {
                    if let Some((row, col)) = g.cell_of(doc.cursor) {
                        match named {
                            NamedKey::Tab if g.step(row, col, !shift).is_some() || shift => return Read,
                            NamedKey::Enter if !shift && g.below(row).is_some() => return Read,
                            NamedKey::Backspace
                                if doc.selection().is_none() && doc.cursor <= g.cells[row][col].start =>
                            {
                                return Ignore
                            }
                            NamedKey::Delete if doc.selection().is_none() && doc.cursor >= g.cells[row][col].end => {
                                return Ignore
                            }
                            _ => {}
                        }
                    }
                }
            }
            if doc.selection().is_none()
                && ((*named == NamedKey::Backspace && doc.cursor == 0)
                    || (*named == NamedKey::Delete && doc.cursor == doc.text.len()))
            {
                Ignore
            } else {
                Edit
            }
        }
        _ if !primary && text.is_some_and(|t| t.chars().any(|c| !c.is_control())) => Edit,
        _ => Ignore,
    }
}

/// Captured once at the pointer's hit-tested byte offset, before the app accepts the date.
/// Selection and caret placement are read-only; checkbox/new-paragraph are edit intent.
#[derive(Debug, PartialEq, Eq)]
pub enum TextPress {
    Link(String),
    Checkbox(usize, usize),
    Word(usize),
    Line(usize),
    Caret(usize, bool),
    LeaveTable(usize),
}
impl TextPress {
    /// Apply the captured target once. Note handles animation and redraw separately.
    pub fn apply(&self, doc: &mut Doc) {
        match *self {
            Self::Link(_) => {}
            Self::Checkbox(ls, le) => {
                doc.toggle_check_at_line(ls, le);
            }
            Self::Word(off) => doc.select_word_at(off),
            Self::Line(off) => doc.select_line_at(off),
            Self::Caret(off, shift) => doc.move_to(off, shift),
            Self::LeaveTable(end) => {
                if doc.text[end..].starts_with('\n') {
                    doc.move_to(end + 1, false);
                } else {
                    doc.replace(end..end, "\n", crate::editor::EditKind::Other);
                }
            }
        }
    }
    pub fn edits(&self) -> bool {
        matches!(self, Self::Checkbox(..) | Self::LeaveTable(..))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::date_view::{DatePreview, Visibility};
    use std::collections::HashMap;
    use winit::event::{DeviceId, MouseButton, MouseScrollDelta, TouchPhase};

    fn k(doc: &Doc, named: NamedKey) -> Route {
        key(doc, &Key::Named(named), None, false, false, None)
    }

    #[test]
    fn focus_scroll_controls_and_selection_leave_preview_cancellable() {
        let day = chrono::NaiveDate::from_ymd_opt(2026, 11, 13).unwrap();
        let original = HashMap::from([
            (1, Visibility { shown: true, minimized: true }),
            (2, Visibility { shown: false, minimized: false }),
        ]);
        let preview = DatePreview { previous: None, original: original.clone() };
        let mut doc = Doc::new("αβ task".into());
        for ev in [
            WindowEvent::Focused(true),
            WindowEvent::Focused(false),
            WindowEvent::CloseRequested,
            WindowEvent::MouseInput {
                device_id: DeviceId::dummy(),
                state: ElementState::Pressed,
                button: MouseButton::Left,
            },
            WindowEvent::MouseInput {
                device_id: DeviceId::dummy(),
                state: ElementState::Pressed,
                button: MouseButton::Right,
            },
            WindowEvent::MouseWheel {
                device_id: DeviceId::dummy(),
                delta: MouseScrollDelta::LineDelta(0.0, 1.0),
                phase: TouchPhase::Moved,
            },
        ] {
            assert_ne!(event(&doc, &ev, ModifiersState::empty(), None), Route::Edit);
        }
        for named in [
            NamedKey::ArrowLeft,
            NamedKey::ArrowRight,
            NamedKey::Home,
            NamedKey::End,
            NamedKey::ArrowUp,
            NamedKey::ArrowDown,
            NamedKey::PageUp,
            NamedKey::PageDown,
            NamedKey::Control,
            NamedKey::F5,
        ] {
            assert_ne!(k(&doc, named), Route::Edit);
        }
        for action in [
            NoteAction::CloseNote,
            NoteAction::DeleteNote,
            NoteAction::NextColor,
            NoteAction::CycleMode,
            NoteAction::NewNote,
            NoteAction::Quit,
            NoteAction::ZoomIn,
        ] {
            assert_eq!(key(&doc, &Key::Character("x".into()), Some("x"), true, false, Some(action)), Route::Ignore);
        }
        let press = TextPress::Caret("α".len(), false);
        assert!(!press.edits());
        press.apply(&mut doc);
        TextPress::Word("αβ ".len()).apply(&mut doc);
        assert_eq!(doc.selected_text(), Some("task"));
        assert_eq!(key(&doc, &Key::Character("c".into()), None, true, false, None), Route::Read);
        let dates = HashMap::from([(1, day), (2, day.pred_opt().unwrap())]);
        preview.choose(Some(day), &dates);
        preview.choose(Some(day.pred_opt().unwrap()), &dates);
        preview.choose(Some(day.succ_opt().unwrap()), &dates);
        // Never Mind restores this original snapshot even after focus, selection and repeated dates.
        assert_eq!(preview.original, original);
        assert_eq!(preview.previous, None);
        assert_eq!(doc.text, "αβ task");
        doc.undo();
        assert_eq!(doc.text, "αβ task", "read-only events must not create undo edits");
    }

    #[test]
    fn first_typing_after_pointer_selection_keeps_exact_unicode_target_and_undo() {
        let mut doc = Doc::new("αβ task".into());
        TextPress::Word("αβ ".len()).apply(&mut doc);
        let text = "書";
        assert_eq!(key(&doc, &Key::Character(text.into()), Some(text), false, false, None), Route::Edit);
        // The prepared dispatch is delivered once after acceptance, without moving the caret.
        doc.insert(text);
        assert_eq!(doc.text, "αβ 書");
        doc.undo();
        assert_eq!(doc.text, "αβ task");
        assert_eq!(doc.selected_text(), Some("task"));
        doc.redo();
        assert_eq!(doc.text, "αβ 書");
    }

    #[test]
    fn checkbox_press_uses_captured_line_once_without_rehit_or_caret_jump() {
        let mut doc = Doc::new("- [ ] α\n- [ ] β".into());
        let start = doc.text.find('\n').unwrap() + 1;
        let press = TextPress::Checkbox(start, doc.text.len());
        assert!(press.edits());
        press.apply(&mut doc);
        assert_eq!(doc.text, "- [ ] α\n- [x] β");
        doc.undo();
        assert_eq!(doc.text, "- [ ] α\n- [ ] β");
    }

    #[test]
    fn ime_start_and_commit_accept_but_empty_composition_does_not() {
        let doc = Doc::new("αβ".into());
        for ime in [Ime::Enabled, Ime::Disabled, Ime::Preedit(String::new(), None), Ime::Commit(String::new())] {
            assert_ne!(event(&doc, &WindowEvent::Ime(ime), ModifiersState::empty(), None), Route::Edit);
        }
        for ime in [Ime::Preedit("書".into(), Some((3, 3))), Ime::Commit("書".into())] {
            assert_eq!(event(&doc, &WindowEvent::Ime(ime), ModifiersState::empty(), None), Route::Edit);
        }
    }

    #[test]
    fn checklist_child_intent_does_not_consume_first_edit() {
        let mut doc = Doc::new("- [x] α".into());
        doc.cursor = doc.text.len();
        assert_eq!(
            key(&doc, &Key::Named(NamedKey::Enter), None, true, false, Some(NoteAction::ToggleTodo)),
            Route::Edit
        );
        assert_eq!(doc.text, "- [x] α");
        assert!(doc.subtask());
        assert_eq!(doc.text, "- [x] α\n  - [ ] ");
        doc.undo();
        assert_eq!(doc.text, "- [x] α");
        doc.preedit = Some(crate::editor::Preedit { text: "書".into(), cursor: Some(3) });
        assert!(!doc.can_subtask());
        assert!(!doc.subtask());
    }

    #[test]
    fn table_navigation_is_read_only_but_row_extension_and_paragraph_accept() {
        let mut doc = Doc::new("| a | b |\n| --- | --- |\n| c | d |".into());
        doc.cursor = 2;
        assert_eq!(k(&doc, NamedKey::Tab), Route::Read);
        assert_eq!(k(&doc, NamedKey::Enter), Route::Read);
        assert_eq!(k(&doc, NamedKey::Backspace), Route::Ignore);
        doc.cursor = doc.text.rfind('d').unwrap();
        assert_eq!(k(&doc, NamedKey::Tab), Route::Edit);
        assert_eq!(k(&doc, NamedKey::Enter), Route::Edit);
        assert_eq!(k(&doc, NamedKey::ArrowDown), Route::Scroll);
        let old = doc.text.clone();
        let press = TextPress::LeaveTable(doc.text.len());
        assert!(press.edits());
        press.apply(&mut doc);
        assert_eq!(doc.text, format!("{old}\n"));
        doc.undo();
        assert_eq!(doc.text, old);
    }

    #[test]
    fn shortcuts_and_altgr_follow_editor_routing() {
        let doc = Doc::new(String::new());
        for c in ["a", "c"] {
            assert_eq!(key(&doc, &Key::Character(c.into()), None, true, false, None), Route::Read);
        }
        for c in ["x", "1", "q"] {
            assert_eq!(key(&doc, &Key::Character(c.into()), None, true, false, None), Route::Ignore);
        }
        assert_eq!(key(&doc, &Key::Character("@".into()), Some("@"), false, false, None), Route::Edit);
        assert_eq!(key(&doc, &Key::Character("b".into()), None, true, false, Some(NoteAction::Bold)), Route::Edit);
        assert_eq!(k(&doc, NamedKey::Backspace), Route::Ignore);
        assert_eq!(k(&doc, NamedKey::Delete), Route::Ignore);
    }
}
