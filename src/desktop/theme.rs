use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::{App, WindowAppearance, px, rgb};
use tiny_proxy::app::appearance::ThemePreference;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub canvas: u32,
    pub surface: u32,
    pub border: u32,
    pub ink: u32,
    pub muted: u32,
    pub warning: u32,
    pub success: u32,
    pub error: u32,
    pub button: u32,
    pub button_hover: u32,
    pub button_active: u32,
    pub primary_hover: u32,
    pub primary_active: u32,
    pub accent: u32,
    pub sidebar: u32,
    pub selected: u32,
    pub soft_border: u32,
    pub primary: u32,
    pub on_primary: u32,
    pub progress: u32,
}

impl Palette {
    pub const fn for_mode(mode: ThemeMode) -> Self {
        match mode {
            ThemeMode::Dark => Self {
                canvas: 0x000000,
                surface: 0x15181e,
                border: 0x3b3d45,
                ink: 0xffffff,
                muted: 0xb2b6bd,
                warning: 0xffcf25,
                success: 0x00ca8e,
                error: 0xf24c53,
                button: 0x1f232b,
                button_hover: 0x3b3d45,
                button_active: 0x15181e,
                primary_hover: 0xe5e7eb,
                primary_active: 0xb2b6bd,
                accent: 0x2b89ff,
                sidebar: 0x15181e,
                selected: 0x1c334c,
                soft_border: 0x252830,
                primary: 0xffffff,
                on_primary: 0x000000,
                progress: 0x00ca8e,
            },
            // Preserve the layout and hierarchy of hashicorp while adapting
            // its dark-only reference to the user's requested daytime mode.
            ThemeMode::Light => Self {
                canvas: 0xf5f5f5,
                surface: 0xffffff,
                border: 0xd1d3d6,
                ink: 0x15181e,
                muted: 0x62676e,
                warning: 0x916000,
                success: 0x006e4f,
                error: 0xc22b36,
                button: 0xffffff,
                button_hover: 0xf0f0f0,
                button_active: 0xe2e2e2,
                primary_hover: 0x353535,
                primary_active: 0x111111,
                accent: 0x1766cf,
                sidebar: 0xffffff,
                selected: 0xf0f0f0,
                soft_border: 0xe1e3e6,
                primary: 0x242424,
                on_primary: 0xffffff,
                progress: 0x35c65a,
            },
        }
    }
}

pub fn resolve(preference: ThemePreference, appearance: WindowAppearance) -> ThemeMode {
    if preference.is_dark(ThemeMode::from(appearance).is_dark()) {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    }
}

pub fn apply(preference: ThemePreference, appearance: WindowAppearance, cx: &mut App) -> Palette {
    let mode = resolve(preference, appearance);
    let colors = Palette::for_mode(mode);
    // Reload the matching component defaults before applying our tokens, so
    // controls and scrollbars cannot keep colors from the previous mode.
    Theme::change(mode, None, cx);
    Theme::update(cx, |theme| {
        theme.font_family = if cfg!(target_os = "macos") {
            ".AppleSystemUIFont"
        } else {
            "Microsoft YaHei UI"
        }
        .into();
        // Root uses this as the rem baseline for controls, icons and spacing.
        // A 14px baseline plus text_sm had reduced body text to 12.25px.
        theme.font_size = px(super::layout::REM_SIZE);
        theme.radius = px(8.);
        theme.shadow = false;
        theme.background = rgb(colors.canvas).into();
        theme.foreground = rgb(colors.ink).into();
        theme.muted_foreground = rgb(colors.muted).into();
        theme.muted = rgb(colors.button).into();
        theme.border = rgb(colors.border).into();
        theme.input = rgb(colors.border).into();
        theme.popover = rgb(colors.surface).into();
        theme.popover_foreground = rgb(colors.ink).into();
        theme.primary = rgb(colors.primary).into();
        theme.primary_foreground = rgb(colors.on_primary).into();
        theme.primary_hover = rgb(colors.primary_hover).into();
        theme.primary_active = rgb(colors.primary_active).into();
        theme.button_primary = theme.primary;
        theme.button_primary_foreground = theme.primary_foreground;
        theme.button_primary_hover = theme.primary_hover;
        theme.button_primary_active = theme.primary_active;
        theme.button = rgb(colors.button).into();
        theme.button_foreground = rgb(colors.ink).into();
        theme.button_hover = rgb(colors.button_hover).into();
        theme.button_active = rgb(colors.button_active).into();
        theme.ring = rgb(colors.accent).into();
        theme.link = rgb(colors.accent).into();
        theme.caret = rgb(colors.ink).into();
        theme.success = rgb(colors.success).into();
        theme.warning = rgb(colors.warning).into();
        theme.danger = rgb(colors.error).into();
    });
    colors
}

#[cfg(test)]
mod tests {
    use super::{Palette, resolve};
    use gpui_kit::WindowAppearance;
    use gpui_kit::component::ThemeMode;
    use tiny_proxy::app::appearance::ThemePreference;

    #[test]
    fn follow_system_handles_normal_and_vibrant_appearances() {
        for (appearance, mode) in [
            (WindowAppearance::Light, ThemeMode::Light),
            (WindowAppearance::VibrantLight, ThemeMode::Light),
            (WindowAppearance::Dark, ThemeMode::Dark),
            (WindowAppearance::VibrantDark, ThemeMode::Dark),
        ] {
            assert_eq!(resolve(ThemePreference::System, appearance), mode);
            assert_eq!(
                resolve(ThemePreference::Light, appearance),
                ThemeMode::Light
            );
            assert_eq!(resolve(ThemePreference::Dark, appearance), ThemeMode::Dark);
        }
    }

    fn luminance(color: u32) -> f64 {
        let channel = |shift: u32| {
            let value = ((color >> shift) & 0xff) as f64 / 255.;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    }

    fn contrast(a: u32, b: u32) -> f64 {
        let a = luminance(a);
        let b = luminance(b);
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn daytime_action_and_selection_colors_are_neutral_in_every_interaction_state() {
        let colors = Palette::for_mode(ThemeMode::Light);
        for color in [
            colors.button,
            colors.button_hover,
            colors.button_active,
            colors.primary,
            colors.primary_hover,
            colors.primary_active,
        ] {
            assert_eq!((color >> 16) & 0xff, (color >> 8) & 0xff);
            assert_eq!((color >> 8) & 0xff, color & 0xff);
        }
    }

    #[test]
    fn both_palettes_keep_text_and_status_readable_on_page_and_panels() {
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            let colors = Palette::for_mode(mode);
            for background in [colors.canvas, colors.surface] {
                for foreground in [
                    colors.ink,
                    colors.muted,
                    colors.warning,
                    colors.success,
                    colors.error,
                ] {
                    assert!(
                        contrast(foreground, background) >= 4.5,
                        "{mode:?}: #{foreground:06x} on #{background:06x}"
                    );
                }
            }
            for background in [colors.button, colors.button_hover, colors.button_active] {
                assert!(contrast(colors.ink, background) >= 4.5);
            }
            for background in [colors.primary, colors.primary_hover, colors.primary_active] {
                assert!(contrast(colors.on_primary, background) >= 4.5);
            }
            assert!(contrast(colors.ink, colors.selected) >= 4.5);
            assert!(contrast(colors.muted, colors.sidebar) >= 4.5);
            assert!(contrast(colors.accent, colors.canvas) >= 4.5);
        }
    }
}
