use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, px, rgb};

pub const CANVAS: u32 = 0x000000;
pub const SURFACE: u32 = 0x15181e;
pub const BORDER: u32 = 0x3b3d45;
pub const INK: u32 = 0xffffff;
pub const MUTED: u32 = 0xb2b6bd;
pub const WARNING: u32 = 0xffcf25;
pub const ERROR: u32 = 0xf24c53;

pub fn apply(cx: &mut App) {
    Theme::change(ThemeMode::Dark, None, cx);
    Theme::update(cx, |theme| {
        theme.font_family = "Microsoft YaHei UI".into();
        theme.font_size = px(14.);
        theme.radius = px(8.);
        theme.shadow = false;
        theme.background = rgb(CANVAS).into();
        theme.foreground = rgb(INK).into();
        theme.muted_foreground = rgb(MUTED).into();
        theme.border = rgb(BORDER).into();
        theme.input = rgb(SURFACE).into();
        theme.primary = rgb(INK).into();
        theme.primary_foreground = rgb(CANVAS).into();
        theme.primary_hover = rgb(0xe5e7eb).into();
        theme.primary_active = rgb(0xb2b6bd).into();
        theme.button_primary = theme.primary;
        theme.button_primary_foreground = theme.primary_foreground;
        theme.button_primary_hover = theme.primary_hover;
        theme.button_primary_active = theme.primary_active;
        theme.button = rgb(0x1f232b).into();
        theme.button_foreground = rgb(INK).into();
        theme.button_hover = rgb(BORDER).into();
        theme.button_active = rgb(SURFACE).into();
        theme.ring = rgb(0x2b89ff).into();
        theme.success = rgb(0x00ca8e).into();
        theme.warning = rgb(WARNING).into();
        theme.danger = rgb(ERROR).into();
    });
}
