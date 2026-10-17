//! Headless page rendering for the pages' own tests.
//!
//! A page whose layout is only ever seen on someone's screen cannot be
//! reviewed for spacing from the code alone — the settings grid was reviewed
//! exactly that way, and its row gap turned out to grow with the window
//! rather than follow the spacing. `egui` is happy to lay a page out with no
//! window behind it: a context, an input rectangle, and the page function are
//! the whole setup, and the painted shapes that come back say what actually
//! happened to the geometry.

use super::*;

use crate::theme::{SURFACE, SURFACE_RAISED};

/// A typical window, matching the app's initial size.
pub(crate) const WINDOW: egui::Vec2 = egui::vec2(1100.0, 700.0);

/// Draw `page` into a context of `size`, the way the app does — a
/// full-width vertical scroll area — and return the painted shapes.
///
/// `theme::install` must have run on `ctx` first, or the page's colours will
/// not match the fill colours these tests look for. Three passes, because the
/// first builds the font atlas and measures nothing reliable, and the
/// geometry counts as measured only once a later pass agrees with it.
pub(crate) fn render_page_shapes(
    ctx: &egui::Context,
    size: egui::Vec2,
    page_draw: impl Fn(&mut egui::Ui, &mut Ctx),
) -> Vec<egui::epaint::ClippedShape> {
    let engine = padcore::engine::EngineHandle::headless();
    let state = std::cell::RefCell::new(crate::state::AppState::new(
        padcore::settings::Settings::default(),
        padcore::profile::ProfileStore::bootstrap(),
    ));

    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::pos2(0.0, 0.0), size)),
        ..egui::RawInput::default()
    };

    let mut shipped: Option<Vec<egui::epaint::ClippedShape>> = None;
    for _ in 0..3 {
        let mut pass = ctx.run_ui(input.clone(), |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_max_width(ui.available_width());
                    let mut page_ctx = Ctx {
                        state: &mut state.borrow_mut(),
                        engine: &engine,
                        input_keys: Vec::new(),
                    };
                    page_draw(ui, &mut page_ctx);
                });
        });
        // Each pass carries its own font-atlas delta, and every pass but the
        // last is dropped here. A test has no renderer to apply it to, so it
        // is cleared rather than tripping epaint's own drop check.
        shipped = Some(std::mem::take(&mut pass.shapes));
        pass.textures_delta.clear();
    }

    shipped.unwrap_or_default()
}

/// The card frames a page painted, by their own fill colour.
///
/// `SURFACE_RAISED` is the primary card; `SURFACE` is the quiet one, which is
/// what the diagnostics page's deliberately spare disconnected state draws.
/// Only frames big enough to be cards count — at least a hundred points wide
/// and thirty tall, the floor being the exact height the diagnostics page's
/// one-line quiet card lands on, which is why the comparison is inclusive.
fn rects_of_fill(shapes: &[egui::epaint::ClippedShape], fill: egui::Color32) -> Vec<egui::Rect> {
    shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::epaint::Shape::Rect(r)
                if r.fill == fill && r.rect.width() > 100.0 && r.rect.height() >= 30.0 =>
            {
                Some(r.rect)
            }
            _ => None,
        })
        .collect()
}

/// The primary cards a page painted.
pub(crate) fn page_cards(
    ctx: &egui::Context,
    size: egui::Vec2,
    page_draw: impl Fn(&mut egui::Ui, &mut Ctx),
) -> Vec<egui::Rect> {
    rects_of_fill(&render_page_shapes(ctx, size, page_draw), SURFACE_RAISED)
}

/// Route to a page's `draw`, mirroring the dispatch in `app.rs`.
///
/// Kept here rather than shared because `App` exists only behind a real
/// engine handle and a tray; the pages themselves are the reusable surface.
fn draw_page_of(ui: &mut egui::Ui, page_ctx: &mut Ctx, page: &crate::state::Page) {
    use crate::state::Page;
    match page {
        Page::Dashboard => dashboard::draw(ui, page_ctx),
        Page::Test => test::draw(ui, page_ctx),
        Page::Controller => controller::draw(ui, page_ctx),
        Page::Racing => racing::draw(ui, page_ctx),
        Page::Mapping => mapping::draw(ui, page_ctx),
        Page::Gyro => gyro::draw(ui, page_ctx),
        Page::Profiles => profiles::draw(ui, page_ctx),
        Page::Output => output::draw(ui, page_ctx),
        Page::Settings => settings::draw(ui, page_ctx),
        Page::Diagnostics => diagnostics::draw(ui, page_ctx),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Page;
    /// Every page must fully lay out with no pad attached and no history.
    ///
    /// That quiet state is half of what a first-time user sees, and it is the
    /// state the diagnostics page shipped in having never been rendered at
    /// all; running it here means a panic in any page's empty state is a test
    /// failure instead of a crash report button.
    #[test]
    fn every_page_renders_without_panicking() {
        let ctx = egui::Context::default();
        for page in Page::ALL {
            page_cards(&ctx, WINDOW, |ui, page_ctx| {
                draw_page_of(ui, page_ctx, page)
            });
        }
    }

    /// And every page draws something: a primary card, or — for the
    /// diagnostics page, whose quiet state is deliberately one message card —
    /// its quiet-card equivalent. A page that somehow painted no frames (a
    /// forgotten return in a broken state, say) is caught here too.
    #[test]
    fn every_page_paints_something() {
        let ctx = egui::Context::default();
        for page in Page::ALL {
            let shapes = render_page_shapes(&ctx, WINDOW, |ui, page_ctx| {
                draw_page_of(ui, page_ctx, page)
            });
            let painted_something = !rects_of_fill(&shapes, SURFACE_RAISED).is_empty()
                || !rects_of_fill(&shapes, SURFACE).is_empty();
            assert!(
                painted_something,
                "{page:?} rendered but painted no card frames"
            );
        }
    }
}
