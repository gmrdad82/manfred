use std::borrow::Cow;

use egui::Color32;
use manfred_scene::fonts::{BOLD, REGULAR};

pub use pfx_editor_style::theme::Theme;
#[cfg(test)]
pub use pfx_editor_style::theme::apply_theme;
use pfx_editor_style::theme::{Caps, Diff, Face, Syntax};

pub const BG_BASE: Color32 = Color32::from_rgb(0x02, 0x26, 0x4A);
pub const BG_DEEP: Color32 = Color32::from_rgb(0x02, 0x1A, 0x39);
pub const BG_CHIP: Color32 = Color32::from_rgb(0x0B, 0x31, 0x59);
pub const BG_SELECTED: Color32 = Color32::from_rgb(0x12, 0x37, 0x56);
pub const LINE_BORDER: Color32 = Color32::from_rgb(0x24, 0x4D, 0x6A);
pub const LINE_SUBTLE: Color32 = Color32::from_rgb(0x1F, 0x3F, 0x5E);
pub const LINE_STRONG: Color32 = Color32::from_rgb(0x36, 0x62, 0x88);
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xDA, 0xE5, 0xF0);
pub const TEXT_CODE: Color32 = Color32::from_rgb(0xA9, 0xD0, 0xE4);
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(0x93, 0xB3, 0xD0);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x65, 0x91, 0xAD);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x4F, 0x72, 0x93);
pub const ACCENT: Color32 = Color32::from_rgb(0xF2, 0xBF, 0x58);
pub const ACCENT_TEXT: Color32 = Color32::from_rgb(0x02, 0x1A, 0x39);
pub const OK: Color32 = Color32::from_rgb(0x5E, 0xB5, 0x86);
pub const OK_TINT: Color32 = Color32::from_rgb(0x0F, 0x42, 0x47);
pub const DANGER: Color32 = Color32::from_rgb(0xE5, 0x66, 0x7A);
pub const DANGER_TINT: Color32 = Color32::from_rgb(0x3B, 0x23, 0x3A);
pub const INFO: Color32 = Color32::from_rgb(0x74, 0xA6, 0xC4);

pub const SIZE_CODE: f32 = 11.0;
pub const SIZE_SMALL: f32 = 12.0;
pub const SIZE_BODY: f32 = 13.0;
pub const SIZE_HEADING: f32 = 16.0;
pub const SIZE_TITLE: f32 = 20.0;

pub const RADIUS: u8 = 2;
pub const STROKE: f32 = 1.0;
pub const PANEL_PADDING: i8 = 10;
pub const ITEM_SPACING: f32 = 6.0;
pub const SECTION_GAP: f32 = 16.0;
pub const DASH: f32 = 2.0;
pub const ROW_HEIGHT: f32 = 19.0;

pub fn manfred() -> Theme {
    Theme {
        name: Cow::Borrowed("manfred"),
        bg: BG_BASE,
        surface: BG_CHIP,
        well: BG_DEEP,
        line: LINE_BORDER,
        track: LINE_SUBTLE,
        text: TEXT_PRIMARY,
        text2: TEXT_SECONDARY,
        muted: TEXT_MUTED,
        dim: TEXT_DIM,
        code: TEXT_CODE,
        on_accent: ACCENT_TEXT,
        accent: LINE_STRONG,
        accent_bright: ACCENT,
        accent_deep: BG_SELECTED,
        secondary: INFO,
        secondary_soft: TEXT_CODE,
        hot: ACCENT,
        hot_dim: BG_CHIP,
        ok: OK,
        ok_tint: OK_TINT,
        error: DANGER,
        error_tint: DANGER_TINT,
        warning: ACCENT,
        syntax: Syntax {
            key: INFO,
            table: ACCENT,
            string: OK,
            number: TEXT_PRIMARY,
            boolean: TEXT_PRIMARY,
            date: TEXT_SECONDARY,
            comment: TEXT_DIM,
            punctuation: TEXT_MUTED,
            plain: TEXT_CODE,
        },
        diff: Diff {
            add: OK_TINT,
            del: DANGER_TINT,
            change: BG_CHIP,
        },
        size_code: SIZE_CODE,
        size_small: SIZE_SMALL,
        size_body: SIZE_BODY,
        size_heading: SIZE_HEADING,
        size_title: SIZE_TITLE,
        radius: RADIUS,
        stroke: STROKE,
        panel_padding: PANEL_PADDING,
        item_spacing: ITEM_SPACING,
        section_gap: SECTION_GAP,
        dash: DASH,
        row_height: ROW_HEIGHT,
        body: Face {
            name: Cow::Borrowed("JetBrainsMonoNerdFont"),
            regular: Cow::Borrowed(REGULAR),
            bold: Cow::Borrowed(BOLD),
        },
        display: None,
        titles: Caps {
            upper: true,
            tracking: 0.0,
        },
        labels: Caps::PLAIN,
    }
}
