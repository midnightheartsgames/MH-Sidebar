use eframe::egui::{
    self, Color32, FontData, FontDefinitions, FontFamily, FontId, RichText, Stroke,
};
use std::sync::Arc;

pub const BG: Color32 = Color32::from_rgb(0x0E, 0x11, 0x16);
pub const PANEL: Color32 = Color32::from_rgb(0x12, 0x16, 0x1D);
pub const CARD: Color32 = Color32::from_rgb(0x15, 0x1A, 0x22);
pub const BORDER: Color32 = Color32::from_rgb(0x24, 0x2C, 0x38);
pub const FIELD: Color32 = Color32::from_rgb(0x0F, 0x13, 0x19);
pub const TEXT: Color32 = Color32::from_rgb(0xF1, 0xF1, 0xF1);
pub const MUTED: Color32 = Color32::from_rgb(0xA9, 0xA9, 0xAD);
pub const ACCENT: Color32 = Color32::from_rgb(0x3F, 0xD0, 0xD8);
pub const WARN: Color32 = Color32::from_rgb(0xF2, 0xA3, 0x3C);
pub const CRITICAL: Color32 = Color32::from_rgb(0xE8, 0x5C, 0x5C);

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("cuprum-bold".into()))
}
pub fn heading(text: impl Into<String>, size: f32) -> RichText {
    RichText::new(text).font(bold(size)).color(TEXT)
}
pub fn setup(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "cuprum".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/Cuprum-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "cuprum-bold".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../assets/fonts/Cuprum-Bold.ttf"
        ))),
    );
    let fallback = fonts.families[&FontFamily::Proportional].clone();
    let mut normal = vec!["cuprum".into()];
    normal.extend(fallback.clone());
    let mut bold = vec!["cuprum-bold".into()];
    bold.extend(fallback);
    fonts.families.insert(FontFamily::Proportional, normal);
    fonts
        .families
        .insert(FontFamily::Name("cuprum-bold".into()), bold);
    ctx.set_fonts(fonts);
    ctx.set_theme(egui::Theme::Dark);
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.panel_fill = BG;
    style.visuals.window_fill = PANEL;
    style.visuals.extreme_bg_color = FIELD;
    style.visuals.selection.bg_fill = Color32::from_rgb(0x21, 0x64, 0x70);
    style.visuals.selection.stroke = Stroke::new(1., ACCENT);
    style.visuals.widgets.inactive.bg_fill = FIELD;
    style.visuals.widgets.inactive.weak_bg_fill = FIELD;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1., BORDER);
    style.visuals.widgets.hovered.bg_fill = CARD;
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1., ACCENT);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(0x26, 0x75, 0x7D);
    style.visuals.widgets.active.bg_stroke = Stroke::new(1., ACCENT);
    style.spacing.item_spacing = egui::vec2(10., 8.);
    style.spacing.button_padding = egui::vec2(12., 7.);
    style.spacing.interact_size.y = 30.;
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(17.));
    style
        .text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(17.));
    style
        .text_styles
        .insert(egui::TextStyle::Small, FontId::proportional(14.));
    ctx.set_style_of(egui::Theme::Dark, style);
}

pub fn card<R>(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let result = egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1., BORDER))
        .corner_radius(7)
        .inner_margin(18)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(heading(title, 19.));
            ui.add_space(9.);
            add(ui)
        })
        .inner;
    ui.add_space(10.);
    result
}

pub fn toggle(ui: &mut egui::Ui, label: &str, value: &mut bool) {
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), 30.),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.label(label);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (rect, mut response) =
                    ui.allocate_exact_size(egui::vec2(46., 24.), egui::Sense::click());
                if response.clicked() {
                    *value = !*value;
                    response.mark_changed();
                }
                response.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::Checkbox,
                        ui.is_enabled(),
                        *value,
                        label,
                    )
                });
                let fill = if *value {
                    ACCENT
                } else {
                    Color32::from_rgb(0x3A, 0x42, 0x50)
                };
                ui.painter().rect_filled(rect, 12, fill);
                let x = if *value {
                    rect.right() - 12.
                } else {
                    rect.left() + 12.
                };
                ui.painter()
                    .circle_filled(egui::pos2(x, rect.center().y), 8., TEXT);
            });
        },
    );
}

pub fn row(ui: &mut egui::Ui, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2((ui.available_width() * 0.48).min(250.), 32.),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.label(label);
            },
        );
        add(ui);
    });
}
