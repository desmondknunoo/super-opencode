//! Colour, type, and spacing tokens. Every view draws from these; none carries its own values.

use std::{fs, sync::Arc};

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Shadow, Stroke,
    TextStyle, Theme, Visuals,
    epaint::text::VariationCoords,
    style::{ScrollStyle, WidgetVisuals},
    vec2,
};

/// Type sizes in points.
pub const TEXT: f32 = 13.5;
pub const SMALL: f32 = 12.0;
pub const READING: f32 = 14.5;
pub const CODE: f32 = 12.5;
pub const TITLE: f32 = 23.0;

/// Corner radii.
pub const RADIUS_CONTROL: u8 = 7;
pub const RADIUS_SURFACE: u8 = 12;

/// Width of the reading column in the transcript and of the composer.
pub const COLUMN: f32 = 760.0;

#[derive(Clone, Copy)]
pub struct Palette {
    /// The work area behind the transcript.
    pub ground: Color32,
    /// The sidebar and status bar.
    pub rail: Color32,
    /// Surfaces that sit on the ground: the composer, a prompt, a table.
    pub raised: Color32,
    pub line: Color32,
    pub line_strong: Color32,
    pub hover: Color32,
    pub ink: Color32,
    pub ink_muted: Color32,
    /// Hints and placeholders. Still at least 4.5:1 on every ground above.
    pub ink_faint: Color32,
    pub accent: Color32,
    /// Text and icons drawn on an accent fill.
    pub on_accent: Color32,
    /// The selected row and selected text.
    pub accent_wash: Color32,
    pub ok: Color32,
    pub warn: Color32,
    pub danger: Color32,
    pub code_ground: Color32,
    pub shadow: Color32,
}

/// Paper tinted toward the logo's blue, with the logo's periwinkle deepened until it
/// reads on white.
const LIGHT: Palette = Palette {
    ground: Color32::from_rgb(0xFB, 0xFB, 0xFE),
    rail: Color32::from_rgb(0xF1, 0xF3, 0xFA),
    raised: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    line: Color32::from_rgb(0xDD, 0xE0, 0xEE),
    line_strong: Color32::from_rgb(0xC4, 0xC9, 0xDE),
    hover: Color32::from_rgb(0xE6, 0xE9, 0xF5),
    ink: Color32::from_rgb(0x14, 0x16, 0x2B),
    ink_muted: Color32::from_rgb(0x4E, 0x53, 0x70),
    ink_faint: Color32::from_rgb(0x63, 0x68, 0x83),
    accent: Color32::from_rgb(0x3F, 0x46, 0xD2),
    on_accent: Color32::from_rgb(0xFF, 0xFF, 0xFF),
    accent_wash: Color32::from_rgb(0xE2, 0xE5, 0xFB),
    ok: Color32::from_rgb(0x1B, 0x7A, 0x49),
    warn: Color32::from_rgb(0x96, 0x58, 0x00),
    danger: Color32::from_rgb(0xBE, 0x2A, 0x3A),
    code_ground: Color32::from_rgb(0xF0, 0xF2, 0xFA),
    shadow: Color32::from_rgba_premultiplied(0x06, 0x08, 0x1C, 0x1C),
};

/// The logo's navy as the ground, with its periwinkle and ice kept close to the mark.
const DARK: Palette = Palette {
    ground: Color32::from_rgb(0x0F, 0x11, 0x27),
    rail: Color32::from_rgb(0x0A, 0x0B, 0x1D),
    raised: Color32::from_rgb(0x18, 0x1B, 0x36),
    line: Color32::from_rgb(0x26, 0x2A, 0x4B),
    line_strong: Color32::from_rgb(0x38, 0x3D, 0x68),
    hover: Color32::from_rgb(0x1C, 0x1F, 0x3E),
    ink: Color32::from_rgb(0xE9, 0xEB, 0xF7),
    ink_muted: Color32::from_rgb(0xAB, 0xAF, 0xCB),
    ink_faint: Color32::from_rgb(0x8F, 0x94, 0xB3),
    accent: Color32::from_rgb(0x8C, 0x94, 0xFA),
    on_accent: Color32::from_rgb(0x0B, 0x0C, 0x1E),
    accent_wash: Color32::from_rgb(0x23, 0x28, 0x5C),
    ok: Color32::from_rgb(0x62, 0xD2, 0x9B),
    warn: Color32::from_rgb(0xF0, 0xB8, 0x68),
    danger: Color32::from_rgb(0xFF, 0x8E, 0x98),
    code_ground: Color32::from_rgb(0x14, 0x17, 0x30),
    shadow: Color32::from_rgba_premultiplied(0, 0, 0, 0x70),
};

impl Palette {
    #[cfg(test)]
    pub fn light() -> Self {
        LIGHT
    }

    pub fn of(ctx: &egui::Context) -> Self {
        match ctx.theme() {
            Theme::Light => LIGHT,
            Theme::Dark => DARK,
        }
    }
}

const STRONG: &str = "strong";

/// The semibold cut of the interface face, for headings and emphasis.
pub fn strong(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(STRONG.into()))
}

pub fn text(size: f32) -> FontId {
    FontId::proportional(size)
}

pub fn mono(size: f32) -> FontId {
    FontId::monospace(size)
}

/// A font file on this system: path, face index inside a collection, and weight on a
/// variable font's `wght` axis.
type Face = (&'static str, u32, Option<f32>);

/// The platform's interface face with its semibold cut, in order of preference. A working
/// screen reads best in the face the rest of the desktop already uses.
#[cfg(target_os = "macos")]
const INTERFACE: &[(Face, Option<Face>)] = &[
    (
        ("/System/Library/Fonts/SFNS.ttf", 0, Some(400.0)),
        Some(("/System/Library/Fonts/SFNS.ttf", 0, Some(600.0))),
    ),
    (
        ("/System/Library/Fonts/HelveticaNeue.ttc", 0, None),
        Some(("/System/Library/Fonts/HelveticaNeue.ttc", 1, None)),
    ),
];
#[cfg(target_os = "macos")]
const MONOSPACE: &[Face] = &[
    ("/System/Library/Fonts/SFNSMono.ttf", 0, Some(400.0)),
    ("/System/Library/Fonts/Menlo.ttc", 0, None),
];
#[cfg(target_os = "macos")]
const CHINESE: &[Face] = &[
    ("/System/Library/Fonts/Hiragino Sans GB.ttc", 0, None),
    ("/System/Library/Fonts/STHeiti Light.ttc", 0, None),
    ("/System/Library/Fonts/PingFang.ttc", 0, None),
];

#[cfg(windows)]
const INTERFACE: &[(Face, Option<Face>)] = &[(
    ("C:\\Windows\\Fonts\\segoeui.ttf", 0, None),
    Some(("C:\\Windows\\Fonts\\seguisb.ttf", 0, None)),
)];
#[cfg(windows)]
const MONOSPACE: &[Face] = &[
    ("C:\\Windows\\Fonts\\CascadiaMono.ttf", 0, Some(400.0)),
    ("C:\\Windows\\Fonts\\consola.ttf", 0, None),
];
#[cfg(windows)]
const CHINESE: &[Face] = &[
    ("C:\\Windows\\Fonts\\msyh.ttc", 0, None),
    ("C:\\Windows\\Fonts\\simsun.ttc", 0, None),
];

#[cfg(all(unix, not(target_os = "macos")))]
const INTERFACE: &[(Face, Option<Face>)] = &[
    (
        (
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
            0,
            None,
        ),
        Some((
            "/usr/share/fonts/truetype/noto/NotoSans-SemiBold.ttf",
            0,
            None,
        )),
    ),
    (
        ("/usr/share/fonts/noto/NotoSans-Regular.ttf", 0, None),
        Some(("/usr/share/fonts/noto/NotoSans-SemiBold.ttf", 0, None)),
    ),
    (
        ("/usr/share/fonts/google-noto/NotoSans-Regular.ttf", 0, None),
        Some((
            "/usr/share/fonts/google-noto/NotoSans-SemiBold.ttf",
            0,
            None,
        )),
    ),
    (
        ("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", 0, None),
        Some((
            "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
            0,
            None,
        )),
    ),
    (
        ("/usr/share/fonts/TTF/DejaVuSans.ttf", 0, None),
        Some(("/usr/share/fonts/TTF/DejaVuSans-Bold.ttf", 0, None)),
    ),
];
#[cfg(all(unix, not(target_os = "macos")))]
const MONOSPACE: &[Face] = &[
    (
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        0,
        None,
    ),
    ("/usr/share/fonts/TTF/DejaVuSansMono.ttf", 0, None),
    (
        "/usr/share/fonts/truetype/noto/NotoSansMono-Regular.ttf",
        0,
        None,
    ),
];
#[cfg(all(unix, not(target_os = "macos")))]
const CHINESE: &[Face] = &[
    (
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        0,
        None,
    ),
    ("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc", 0, None),
    (
        "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
        0,
        None,
    ),
    ("/usr/share/fonts/truetype/wqy/wqy-microhei.ttc", 0, None),
];

fn load((path, index, weight): Face) -> Option<FontData> {
    let mut data = FontData::from_owned(fs::read(path).ok()?);
    data.index = index;
    if let Some(weight) = weight {
        data.tweak.coords = VariationCoords::new([("wght", weight)]);
    }
    Some(data)
}

fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    let bundled_text = fonts.families[&FontFamily::Proportional].clone();
    let bundled_mono = fonts.families[&FontFamily::Monospace].clone();
    let mut add = |name: &str, data: FontData| {
        fonts.font_data.insert(name.to_owned(), Arc::new(data));
        name.to_owned()
    };

    let mut regular = Vec::new();
    let mut semibold = Vec::new();
    for (face, strong_face) in INTERFACE {
        if let Some(data) = load(*face) {
            regular.push(add("interface", data));
            if let Some(data) = strong_face.and_then(load) {
                semibold.push(add("interface-strong", data));
            }
            break;
        }
    }
    let mut monospace: Vec<String> = MONOSPACE
        .iter()
        .find_map(|face| load(*face))
        .map(|data| add("code", data))
        .into_iter()
        .collect();
    // The models this app runs often answer in Chinese, which the bundled faces cannot draw.
    let chinese = CHINESE
        .iter()
        .find_map(|face| load(*face))
        .map(|data| add("chinese", data));

    // Without a semibold cut, emphasis falls back to the regular face and relies on size.
    if semibold.is_empty() {
        semibold = regular.clone();
    }
    for family in [&mut regular, &mut semibold] {
        family.extend(bundled_text.iter().cloned());
        family.extend(chinese.iter().cloned());
    }
    monospace.extend(bundled_mono);
    monospace.extend(chinese);

    fonts.families.insert(FontFamily::Proportional, regular);
    fonts.families.insert(FontFamily::Monospace, monospace);
    fonts
        .families
        .insert(FontFamily::Name(STRONG.into()), semibold);
    fonts
}

fn widget(fill: Color32, line: Color32, ink: Color32) -> WidgetVisuals {
    WidgetVisuals {
        bg_fill: fill,
        weak_bg_fill: fill,
        bg_stroke: Stroke::new(1.0, line),
        corner_radius: CornerRadius::same(RADIUS_CONTROL),
        fg_stroke: Stroke::new(1.0, ink),
        // Controls keep their size on hover and press.
        expansion: 0.0,
    }
}

fn visuals(theme: Theme, palette: &Palette) -> Visuals {
    let mut visuals = match theme {
        Theme::Light => Visuals::light(),
        Theme::Dark => Visuals::dark(),
    };
    visuals.panel_fill = palette.ground;
    visuals.window_fill = palette.raised;
    visuals.window_stroke = Stroke::new(1.0, palette.line);
    visuals.window_corner_radius = CornerRadius::same(RADIUS_SURFACE);
    visuals.menu_corner_radius = CornerRadius::same(RADIUS_CONTROL + 2);
    let shadow = Shadow {
        offset: [0, 6],
        blur: 20,
        spread: 0,
        color: palette.shadow,
    };
    visuals.window_shadow = shadow;
    visuals.popup_shadow = shadow;
    visuals.extreme_bg_color = palette.raised;
    visuals.text_edit_bg_color = Some(palette.raised);
    visuals.faint_bg_color = palette.rail;
    visuals.code_bg_color = palette.code_ground;
    visuals.hyperlink_color = palette.accent;
    visuals.warn_fg_color = palette.warn;
    visuals.error_fg_color = palette.danger;
    visuals.weak_text_color = Some(palette.ink_muted);
    visuals.selection.bg_fill = palette.accent_wash;
    visuals.selection.stroke = Stroke::new(1.5, palette.accent);
    visuals.text_cursor.stroke = Stroke::new(1.5, palette.accent);
    visuals.widgets.noninteractive = widget(palette.ground, palette.line, palette.ink);
    visuals.widgets.inactive = widget(palette.raised, palette.line, palette.ink);
    visuals.widgets.hovered = widget(palette.hover, palette.line_strong, palette.ink);
    visuals.widgets.active = widget(palette.accent_wash, palette.line_strong, palette.ink);
    visuals.widgets.open = widget(palette.hover, palette.line_strong, palette.ink);
    visuals
}

/// Installs the faces, the type scale, and both colour themes.
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    ctx.all_styles_mut(|style| {
        style.text_styles = [
            (TextStyle::Body, text(TEXT)),
            (TextStyle::Button, text(TEXT)),
            (TextStyle::Small, text(SMALL)),
            (TextStyle::Heading, strong(TITLE)),
            (TextStyle::Monospace, mono(CODE)),
        ]
        .into();
        style.spacing.item_spacing = vec2(8.0, 6.0);
        style.spacing.button_padding = vec2(11.0, 5.0);
        style.spacing.interact_size.y = 28.0;
        style.spacing.scroll = ScrollStyle::thin();
        style.animation_time = 0.12;
    });
    ctx.set_visuals_of(Theme::Light, visuals(Theme::Light, &LIGHT));
    ctx.set_visuals_of(Theme::Dark, visuals(Theme::Dark, &DARK));
}
