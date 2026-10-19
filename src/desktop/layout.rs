//! Layout shared by every page. GPUI's h_flex centers children by default.

use super::*;

pub(super) const REM_SIZE: f32 = 16.;
pub(super) const BODY_SIZE: f32 = 15.;
pub(super) const LABEL_SIZE: f32 = 14.;
pub(super) const TITLE_SIZE: f32 = 26.;

pub(super) fn page_title(title: &'static str) -> Div {
    // A zero minimum width needs flex growth, or intrinsic sizing can collapse
    // this column to a single CJK glyph beside the fixed-width actions.
    div()
        .flex_1()
        .min_w_0()
        .truncate()
        .text_size(px(TITLE_SIZE))
        .line_height(relative(1.2))
        .font_weight(FontWeight::BOLD)
        .child(title)
}

pub(super) fn workspace() -> Div {
    div().h_flex().items_stretch().flex_1().min_h_0()
}

pub(super) fn page_frame() -> Div {
    div()
        .h_flex()
        .items_stretch()
        .relative()
        .size_full()
        .min_h_0()
}

pub(super) fn sidebar_frame() -> Div {
    div()
        .v_flex()
        .self_stretch()
        .w(px(208.))
        .flex_shrink_0()
        .min_h_0()
        .m_2()
        .p_2()
        .gap_2()
}

pub(super) fn table_column(width: f32) -> Div {
    div()
        .w(px(width))
        .min_w(px(width))
        .flex_grow(width)
        .flex_shrink_0()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_kit::{AlignItems, AlignSelf, Styled};

    #[test]
    fn workspace_and_drawer_frames_stretch_columns_to_the_top_and_bottom() {
        assert_eq!(workspace().style().align_items, Some(AlignItems::Stretch));
        assert_eq!(page_frame().style().align_items, Some(AlignItems::Stretch));
        // Without cross-axis stretch, a sidebar uses its content height and is centered.
        assert_eq!(sidebar_frame().style().align_self, Some(AlignSelf::Stretch));
    }

    #[test]
    fn page_titles_take_remaining_width_and_never_wrap_into_a_vertical_column() {
        for title in PAGES {
            let mut heading = page_title(title);
            let style = heading.style();
            assert_eq!(style.flex_grow, Some(1.));
            assert_eq!(style.min_size.width, Some(px(0.).into()));
            assert_eq!(style.text.white_space, Some(WhiteSpace::Nowrap));
        }
    }

    #[test]
    fn table_columns_share_extra_width_instead_of_leaving_it_after_the_actions() {
        assert_eq!(table_column(160.).style().flex_grow, Some(160.));
        assert_eq!(table_column(64.).style().flex_grow, Some(64.));
        assert_eq!(table_column(160.).style().flex_shrink, Some(0.));
    }
}
