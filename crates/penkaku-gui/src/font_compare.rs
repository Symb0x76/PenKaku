//! 字体对比: the same text in up to four library fonts, one row each, so fonts can be compared
//! without switching the document's font back and forth.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme as _, Disableable as _, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use penkaku_core::font::Font;
use penkaku_core::layout::EM_UNITS;

use crate::font_preview::{FontPreview, preview};
use crate::state::AppState;

/// Fonts compared at once.
pub const MAX_PICKS: usize = 4;
/// Default text when the caller has none.
const SAMPLE: &str = "永和九年，岁在癸丑，暮春之初";
/// Row height (one em) and the widest a row may be, px.
const ROW_H: f32 = 44.0;
const ROW_W: f32 = 520.0;

pub struct CompareView {
    /// (id, name) of the fonts that can be picked, in library order.
    fonts: Vec<(i64, String)>,
    picked: Vec<i64>,
    text: Entity<InputState>,
    loaded: HashMap<i64, Arc<Font>>,
    loading: HashSet<i64>,
    _subs: Vec<Subscription>,
}

/// The first `MAX_PICKS` of `want` that are in `fonts`, in `want`'s order, no repeats.
pub fn initial_picks(fonts: &[(i64, String)], want: &[i64]) -> Vec<i64> {
    let mut out: Vec<i64> = Vec::new();
    for &id in want {
        if out.len() < MAX_PICKS && !out.contains(&id) && fonts.iter().any(|f| f.0 == id) {
            out.push(id);
        }
    }
    out
}

impl CompareView {
    fn new(fonts: Vec<(i64, String)>, want: &[i64], window: &mut Window, cx: &mut Context<Self>) -> Self {
        let text = cx.new(|cx| InputState::new(window, cx).default_value(SAMPLE).placeholder("对比用的文字"));
        let subs = vec![cx.subscribe_in(&text, window, |_t, _s, ev: &InputEvent, _w, cx| {
            if matches!(ev, InputEvent::Change) {
                cx.notify();
            }
        })];
        let picked = initial_picks(&fonts, want);
        let mut this = Self { fonts, picked: Vec::new(), text, loaded: HashMap::new(), loading: HashSet::new(), _subs: subs };
        for id in picked {
            this.toggle(id, cx);
        }
        this
    }

    /// Pick or drop font `id`; a newly picked font loads off the UI thread.
    fn toggle(&mut self, id: i64, cx: &mut Context<Self>) {
        if let Some(k) = self.picked.iter().position(|&p| p == id) {
            self.picked.remove(k);
        } else if self.picked.len() < MAX_PICKS {
            self.picked.push(id);
            if !self.loaded.contains_key(&id) && self.loading.insert(id) {
                let store = cx.global::<AppState>().fonts.clone();
                let task = cx.background_executor().spawn(async move { store.load(id) });
                cx.spawn(async move |this, cx| {
                    let font = task.await;
                    let _ = this.update(cx, |t, cx| {
                        t.loading.remove(&id);
                        match font {
                            Ok(f) => {
                                t.loaded.insert(id, Arc::new(f));
                            }
                            Err(e) => log::warn!("字体对比: font {id}: {e}"),
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
        }
        cx.notify();
    }
}

impl Render for CompareView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (fg, muted, border) = (theme.foreground, theme.muted_foreground, theme.border);
        let text = self.text.read(cx).value().to_string();
        let full = self.picked.len() >= MAX_PICKS;
        let list = self.fonts.iter().map(|(id, name)| {
            let (id, on) = (*id, self.picked.contains(id));
            Checkbox::new(("cmp-pick", id as usize))
                .small()
                .label(name.clone())
                .checked(on)
                .disabled(full && !on)
                .on_click(cx.listener(move |t, _: &bool, _w, cx| t.toggle(id, cx)))
                .into_any_element()
        });
        let rows = self.picked.iter().map(|id| {
            let name = self.fonts.iter().find(|f| f.0 == *id).map_or_else(String::new, |f| f.1.clone());
            let (face, note) = match self.loaded.get(id) {
                None => (div().text_xs().text_color(muted).child("载入中…").into_any_element(), String::new()),
                Some(font) => match FontPreview::of_text(font, &text) {
                    (Some(p), missing) => {
                        let h = ROW_H.min(ROW_W * EM_UNITS / p.width().max(0.1));
                        (preview(Arc::new(p), h, fg).into_any_element(), if missing > 0 { format!("缺 {missing} 字") } else { String::new() })
                    }
                    (None, _) => (div().text_xs().text_color(muted).child("这些字都没有").into_any_element(), String::new()),
                },
            };
            v_flex()
                .gap_1()
                .py_2()
                .border_b_1()
                .border_color(border)
                .child(h_flex().gap_2().text_xs().child(div().font_weight(FontWeight::SEMIBOLD).child(name)).child(div().text_color(theme.danger).child(note)))
                .child(div().h(px(ROW_H)).flex().items_center().child(face))
                .into_any_element()
        });
        let empty = self.picked.is_empty();
        h_flex()
            .gap_4()
            .items_start()
            .h(px(380.))
            .child(
                v_flex()
                    .id("cmp-fonts")
                    .w(px(180.))
                    .h_full()
                    .gap_1()
                    .overflow_y_scroll()
                    .child(div().text_xs().text_color(muted).child(format!("勾选最多 {MAX_PICKS} 个字体")))
                    .children(list),
            )
            .child(
                v_flex()
                    .id("cmp-rows")
                    .flex_1()
                    .h_full()
                    .gap_1()
                    .overflow_y_scroll()
                    .child(Input::new(&self.text).small())
                    .children(rows)
                    .when(empty, |el| el.child(div().pt_4().text_sm().text_color(muted).child("在左侧勾选字体"))),
            )
    }
}

/// 字体库 › 对比字体…: `fonts` are the pickable (id, name); `want` the ones ticked at first.
pub fn open(fonts: Vec<(i64, String)>, want: Vec<i64>, window: &mut Window, cx: &mut App) {
    let view = cx.new(|cx| CompareView::new(fonts, &want, window, cx));
    window.open_dialog(cx, move |d, _w, _cx| d.title("字体对比").w(px(760.)).child(view.clone()));
}

#[cfg(test)]
mod tests {
    use super::{MAX_PICKS, initial_picks};

    #[test]
    fn initial_picks_keep_order_skip_unknown_and_cap() {
        let fonts: Vec<(i64, String)> = (1..=6).map(|i| (i, format!("f{i}"))).collect();
        assert_eq!(initial_picks(&fonts, &[3, 9, 3, 1]), vec![3, 1]);
        assert_eq!(initial_picks(&fonts, &[6, 5, 4, 3, 2, 1]), vec![6, 5, 4, 3]);
        assert_eq!(initial_picks(&fonts, &[1, 2, 3, 4, 5]).len(), MAX_PICKS);
        assert!(initial_picks(&fonts, &[]).is_empty());
    }
}
