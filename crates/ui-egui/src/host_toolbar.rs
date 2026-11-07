//! Optional Android assistant state, integrated into the native ribbon tabs.
#[derive(Default)]
pub struct HostToolbar {
    pub action: Option<&'static str>,
    pub assistant_open: bool,
    pub busy: bool,
    pub status: String,
}

/// Help › Check for Updates: true when the host checks its own releases instead (a build with
/// the assistant must not be replaced by the original project's, which has none).
pub fn check_for_updates(app: &mut crate::SheetApp) -> bool {
    match &mut app.services.host_toolbar {
        Some(host) => {
            host.action = Some("check_update");
            true
        }
        None => false,
    }
}

// ---------------------------------------------------------------------------------------------
// The ribbon's pin, as in Word: «Collapse the Ribbon» (a chevron at the ribbon's bottom corner)
// hides the groups and leaves the tabs; a tab clicked then opens the ribbon for a look, which
// closes again on a click in the document (or Esc), and its pin («Pin the ribbon») keeps it open.
//
// All of it lives here, in a file the original project does not have, so merging it never
// conflicts here. The original's ribbon calls in only twice: `tab_clicked` when a tab is clicked
// and `ribbon_pin` as the last thing drawn in the ribbon. The collapse itself is the original's
// own state (`ribbon_collapsed`, Ctrl+F1): this adds the button and the look, nothing else.
// ---------------------------------------------------------------------------------------------

use std::cell::Cell;

/// `PEEK` before the ribbon has been drawn for the look.
const PEEK_STARTING: u64 = u64::MAX;

thread_local! {
    /// The pass the open look began (the ribbon was opened by a tab click while collapsed).
    static PEEK: Cell<Option<u64>> = const { Cell::new(None) };
    /// The last pass the ribbon was drawn.
    static SHOWN: Cell<u64> = const { Cell::new(0) };
}

/// A tab was clicked while the ribbon was `collapsed`: what the original then does (open it)
/// becomes a look rather than pinning it back.
fn start_peek(collapsed: bool) {
    if collapsed {
        PEEK.with(|p| p.set(Some(PEEK_STARTING)));
    }
}

/// The pin (or, while pinned, the collapse chevron) in a corner of `area`, the ribbon's groups;
/// `left` puts it in the bottom-left corner (right-to-left interfaces). Also ends a look on a
/// press in the document or Esc. Changes `collapsed` only as the user asks.
fn pin_button(ui: &mut egui::Ui, collapsed: &mut bool, area: egui::Rect, left: bool, spanish: bool) {
    use egui::{Sense, Stroke, pos2, vec2};
    let ctx = ui.ctx().clone();
    let pass = ctx.cumulative_pass_nr();
    let shown_before = SHOWN.with(|s| s.replace(pass));
    let mut peek = PEEK.with(Cell::get);
    match peek {
        Some(PEEK_STARTING) => peek = Some(pass),
        // The ribbon was hidden in between (Ctrl+F1 twice): it was pinned back by hand.
        Some(_) if shown_before.saturating_add(1) < pass => peek = None,
        _ => {}
    }
    if *collapsed || !area.is_positive() {
        PEEK.with(|p| p.set(peek));
        return;
    }
    let size = 22.0;
    let x = if left { area.left() + 2.0 } else { area.right() - size - 2.0 };
    let rect = egui::Rect::from_min_size(pos2(x, area.bottom() - size - 1.0), vec2(size, size));
    let response = ui.interact(rect, egui::Id::unique("host_toolbar_ribbon_pin"), Sense::click());
    let shortcut = if cfg!(target_os = "macos") { "Cmd+F1" } else { "Ctrl+F1" };
    let label = match (peek.is_some(), spanish) {
        (true, true) => "Fijar la cinta".to_string(),
        (true, false) => "Pin the ribbon".to_string(),
        (false, true) => format!("Contraer la cinta ({shortcut})"),
        (false, false) => format!("Collapse the Ribbon ({shortcut})"),
    };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    let visuals = ui.visuals();
    let color = if response.hovered() { visuals.strong_text_color() } else { visuals.text_color() };
    if response.hovered() {
        ui.painter().rect_filled(rect, 4.0, visuals.widgets.hovered.weak_bg_fill);
    }
    let stroke = Stroke::new(1.4, color);
    let c = rect.center();
    let painter = ui.painter();
    if peek.is_some() {
        // A push pin, drawn here: head, body, collar and point.
        painter.rect_stroke(egui::Rect::from_min_max(pos2(c.x - 3.5, c.y - 7.0), pos2(c.x + 3.5, c.y - 5.0)), 1.0, stroke, egui::StrokeKind::Middle);
        painter.rect_stroke(egui::Rect::from_min_max(pos2(c.x - 2.0, c.y - 5.0), pos2(c.x + 2.0, c.y + 0.5)), 0.0, stroke, egui::StrokeKind::Middle);
        painter.line_segment([pos2(c.x - 5.5, c.y + 0.5), pos2(c.x + 5.5, c.y + 0.5)], stroke);
        painter.line_segment([pos2(c.x, c.y + 0.5), pos2(c.x, c.y + 7.0)], stroke);
    } else {
        // A chevron pointing up: the groups fold away.
        painter.line_segment([pos2(c.x - 5.0, c.y + 2.5), pos2(c.x, c.y - 2.5)], stroke);
        painter.line_segment([pos2(c.x, c.y - 2.5), pos2(c.x + 5.0, c.y + 2.5)], stroke);
    }
    let response = response.on_hover_text(&label);
    if response.clicked() {
        if peek.is_some() {
            // Pinned: the ribbon stays, the look is over.
            peek = None;
        } else {
            *collapsed = true;
        }
    } else if peek.is_some() {
        // The look closes on a press outside the ribbon in the window's own panels (the document),
        // never on a menu or dialog the ribbon opened, or on Esc.
        // Read the press first: asking the context for layers inside `input` would lock it twice.
        // From the events: a quick click can press and release within one frame.
        let press = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::PointerButton { pos, pressed: true, .. } => Some(*pos),
                _ => None,
            })
        });
        let pressed_outside =
            press.is_some_and(|p| p.y > area.bottom() + 2.0 && ctx.layer_id_at(p).is_some_and(|l| l.order == egui::Order::Background));
        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape)) && !ctx.any_popup_open();
        if (pressed_outside && !ctx.any_popup_open()) || escape {
            *collapsed = true;
            peek = None;
        }
    }
    PEEK.with(|p| p.set(peek));
}

/// A tab (`tab`) was clicked: the original then opens a collapsed ribbon, which becomes a look.
pub fn tab_clicked(app: &crate::SheetApp, tab: &str) {
    start_peek(app.ui.ribbon_collapsed && !matches!(tab, "Asistente" | "Ayuda"));
}

/// The ribbon's pin or collapse button, drawn last in the ribbon over its card.
pub fn ribbon_pin(ui: &mut egui::Ui, app: &mut crate::SheetApp, card: egui::Rect) {
    let spanish = app.ui.language == crate::i18n::Language::Es;
    pin_button(ui, &mut app.ui.ribbon_collapsed, card.shrink2(egui::vec2(4.0, 2.0)), false, spanish);
}

#[cfg(test)]
mod pin_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use egui_kittest::kittest::Queryable;

    /// The pin, whichever language the system gave the interface.
    fn pin_label<'a>(h: &'a egui_kittest::Harness<'_, crate::SheetApp>) -> Option<egui_kittest::Node<'a>> {
        h.query_by_label("Pin the ribbon").or_else(|| h.query_by_label("Fijar la cinta"))
    }

    #[test]
    fn the_ribbon_collapses_opens_for_a_look_and_pins_back() {
        let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1400.0, 800.0)).build_ui_state(
            |ui, app: &mut crate::SheetApp| {
                app.logic(ui.ctx());
                app.ui(ui);
            },
            {
                let mut session = gridcraft_engine::Session::new();
                session.new_workbook();
                crate::SheetApp::new(session, crate::Services { host_toolbar: Some(Default::default()), ..Default::default() })
            },
        );
        h.run_steps(3);
        // Pinned: the chevron (labelled with Ctrl+F1) collapses it.
        h.get_by_label_contains("+F1)").click();
        h.run_steps(2);
        assert!(h.state().ui.ribbon_collapsed);
        assert!(h.query_by_label_contains("+F1)").is_none());
        // A tab opens it for a look, with the pin.
        h.get_by_label("Insert").click();
        h.run_steps(2);
        assert!(!h.state().ui.ribbon_collapsed);
        assert_eq!(h.state().ui.ribbon_tab, "Insert");
        // A press in the document ends the look.
        let below = egui::pos2(700.0, 700.0);
        h.input_mut().events.push(egui::Event::PointerMoved(below));
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: below,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        });
        h.input_mut().events.push(egui::Event::PointerButton {
            pos: below,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        });
        h.run_steps(2);
        assert!(h.state().ui.ribbon_collapsed);
        h.get_by_label("Insert").click();
        h.run_steps(2);
        assert!(!h.state().ui.ribbon_collapsed);
        pin_label(&h).expect("the pin").click();
        h.run_steps(2);
        // Pinned: it stays, and the chevron is back.
        assert!(!h.state().ui.ribbon_collapsed);
        assert!(pin_label(&h).is_none());
        assert!(h.query_by_label_contains("+F1)").is_some());
    }
}
