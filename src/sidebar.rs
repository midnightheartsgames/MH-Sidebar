use crate::theme;
use eframe::egui::{self, Color32, RichText, Stroke, vec2};
use mh_sidebar::{
    config::Settings,
    history::History,
    model::{Block, Reading, Snapshot},
};
use std::{collections::HashMap, time::Duration};

pub type Histories = HashMap<String, History>;
#[derive(Default)]
pub struct Actions {
    pub settings: bool,
    pub hide: bool,
    pub lock: bool,
}
pub fn key(block: Block, device: &str, row: &str) -> String {
    format!("{block:?}/{device}/{row}")
}

pub fn show(
    ui: &mut egui::Ui,
    snapshot: &Snapshot,
    settings: &Settings,
    histories: &Histories,
    now: f64,
    interactive: bool,
) -> Actions {
    let mut actions = Actions::default();
    // Register the background hit area first so later header buttons and scroll
    // controls remain above it in egui's hit-test order.
    let background = interactive.then(|| {
        ui.interact(
            ui.max_rect(),
            ui.id().with("sidebar-menu"),
            egui::Sense::click(),
        )
    });
    let accent = Color32::from_rgb(settings.accent[0], settings.accent[1], settings.accent[2]);
    ui.style_mut().spacing.item_spacing = vec2(6., 2.);
    ui.style_mut().spacing.interact_size.y = settings.font_size + 4.;
    ui.style_mut().spacing.button_padding = vec2(7., 2.);
    ui.style_mut().text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::proportional(settings.font_size),
    );
    if settings.show_header {
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(18., 20.), egui::Sense::hover());
            for (i, h) in [7., 16., 11.].iter().enumerate() {
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(rect.left() + i as f32 * 6., rect.bottom() - h),
                        vec2(4., *h),
                    ),
                    1,
                    accent,
                );
            }
            ui.label(
                RichText::new("MH SIDEBAR")
                    .font(theme::bold(15.))
                    .color(theme::MUTED)
                    .extra_letter_spacing(1.3),
            );
            if interactive {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button("×")
                        .on_hover_text("Скрыть панель; вернуть через трей")
                        .clicked()
                    {
                        actions.hide = true;
                    }
                    if ui.small_button("⚙").on_hover_text("Настройки").clicked() {
                        actions.settings = true;
                    }
                });
            }
        });
        ui.add_space(5.);
    }
    for block in &settings.blocks {
        if !block.enabled {
            continue;
        }
        if block.id == Block::Clock {
            clock(ui, settings, accent);
            separator(ui);
            continue;
        }
        let sections: Vec<_> = snapshot
            .sections
            .iter()
            .filter(|s| {
                s.id == block.id && (block.devices.is_empty() || block.devices.contains(&s.device))
            })
            .collect();
        if sections.is_empty() {
            ui.label(theme::heading(block.id.short(), settings.font_size + 2.));
            ui.label(
                RichText::new(if snapshot.sections.is_empty() {
                    "Чтение показателей…"
                } else {
                    "Устройство недоступно"
                })
                .color(theme::MUTED),
            );
            separator(ui);
            continue;
        }
        for section in sections {
            // Reserve the paint slot before labels: the graph stays behind text
            // and uses the section's measured height without adding layout space.
            let graph_slot = ui.painter().add(egui::Shape::Noop);
            let section_start = ui.next_widget_position();
            let section_width = ui.available_width();
            let main = section
                .rows
                .iter()
                .find(|r| r.key == "load" && block.shows(&r.key));
            ui.horizontal(|ui| {
                ui.label(theme::heading(section.id.short(), settings.font_size + 3.));
                if let Some(main) = main {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(&main.text)
                                .font(theme::bold(settings.font_size + 3.))
                                .color(accent),
                        );
                    });
                }
            });
            if block.show_name {
                ui.label(
                    RichText::new(&section.device)
                        .size(settings.font_size - 2.)
                        .color(theme::MUTED),
                );
            }
            for row in &section.rows {
                if row.key == "load"
                    || !block.shows(&row.key)
                    || (!settings.show_cores && row.key.starts_with("core_"))
                    || (settings.hide_unavailable && row.value.is_none())
                {
                    continue;
                }
                reading(ui, row, settings);
            }
            if block.graph
                && let Some(row) = section.rows.iter().find(|r| r.key == block.graph_metric)
                && let Some(history) = histories.get(&key(section.id, &section.device, &row.key))
            {
                let rect = egui::Rect::from_min_max(
                    section_start,
                    egui::pos2(
                        section_start.x + section_width,
                        ui.next_widget_position().y - ui.spacing().item_spacing.y,
                    ),
                );
                ui.painter().set(
                    graph_slot,
                    graph(
                        rect,
                        history,
                        now,
                        settings.graph_seconds,
                        accent,
                        row.unit == "%",
                    ),
                );
            }
            separator(ui);
        }
    }
    if let Some(response) = background {
        response.context_menu(|ui| {
            if ui.button("Настройки…").clicked() {
                actions.settings = true;
                ui.close();
            }
            if ui.button("Пропускать клики").clicked() {
                actions.lock = true;
                ui.close();
            }
            if ui.button("Скрыть панель").clicked() {
                actions.hide = true;
                ui.close();
            }
        });
    }
    actions
}

fn reading(ui: &mut egui::Ui, row: &Reading, settings: &Settings) {
    let color = if settings.alerts && row.key == "temperature" {
        if row
            .value
            .is_some_and(|v| v >= settings.critical_temperature)
        {
            theme::CRITICAL
        } else if row.value.is_some_and(|v| v >= settings.warning_temperature) {
            theme::WARN
        } else {
            theme::TEXT
        }
    } else {
        theme::TEXT
    };
    let response = ui
        .horizontal(|ui| {
            let available = ui.available_width();
            ui.allocate_ui_with_layout(
                vec2(available * 0.54, settings.font_size + 2.),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.add(
                        egui::Label::new(RichText::new(&row.label).color(theme::MUTED)).truncate(),
                    );
                },
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.set_max_width(available * 0.55);
                ui.label(RichText::new(&row.text).color(color));
            });
        })
        .response;
    if let Some(reason) = &row.reason {
        response.on_hover_text(reason);
    } else {
        response.on_hover_text(&row.label);
    }
}

fn separator(ui: &mut egui::Ui) {
    ui.add_space(3.);
    ui.separator();
    ui.add_space(3.);
}
fn clock(ui: &mut egui::Ui, settings: &Settings, accent: Color32) {
    use windows_sys::Win32::{Foundation::SYSTEMTIME, System::SystemInformation::GetLocalTime};
    let mut time: SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe { GetLocalTime(&mut time) };
    let hour = if settings.clock_24h {
        time.wHour
    } else {
        let h = time.wHour % 12;
        if h == 0 { 12 } else { h }
    };
    let mut text = format!("{hour:02}:{:02}", time.wMinute);
    if settings.show_seconds {
        text.push_str(&format!(":{:02}", time.wSecond));
    }
    if !settings.clock_24h {
        text.push_str(if time.wHour >= 12 { " PM" } else { " AM" });
    }
    ui.label(RichText::new(text).font(theme::bold(32.)).color(accent));
    if settings.show_date {
        let months = [
            "января",
            "февраля",
            "марта",
            "апреля",
            "мая",
            "июня",
            "июля",
            "августа",
            "сентября",
            "октября",
            "ноября",
            "декабря",
        ];
        ui.label(
            RichText::new(format!(
                "{} {} {}",
                time.wDay,
                months[(time.wMonth.saturating_sub(1) as usize).min(11)],
                time.wYear
            ))
            .color(theme::MUTED),
        );
    }
}

fn graph(
    rect: egui::Rect,
    history: &History,
    now: f64,
    seconds: u64,
    accent: Color32,
    percent: bool,
) -> egui::Shape {
    let points = history.points(now, Duration::from_secs(seconds));
    let max = if percent {
        100.
    } else {
        points.iter().filter_map(|(_, v)| *v).fold(1., f64::max) * 1.15
    };
    let mut shapes = Vec::new();
    let mut mesh = egui::Mesh::default();
    let fill = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 18);
    let stroke = Stroke::new(
        1.,
        Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 65),
    );
    let mut line = Vec::new();
    for (t, value) in points {
        if let Some(v) = value {
            let x = rect.left()
                + ((t - (now - seconds as f64)) / seconds as f64).clamp(0., 1.) as f32
                    * rect.width();
            let y = rect.bottom() - (v / max).clamp(0., 1.) as f32 * (rect.height() - 2.);
            let point = egui::pos2(x, y);
            if let Some(previous) = line.last().copied() {
                let base = mesh.vertices.len() as u32;
                for p in [
                    previous,
                    point,
                    egui::pos2(x, rect.bottom()),
                    egui::pos2(previous.x, rect.bottom()),
                ] {
                    mesh.colored_vertex(p, fill);
                }
                mesh.add_triangle(base, base + 1, base + 2);
                mesh.add_triangle(base, base + 2, base + 3);
            }
            line.push(point);
        } else if line.len() > 1 {
            shapes.push(egui::Shape::line(std::mem::take(&mut line), stroke));
        } else {
            line.clear();
        }
    }
    if line.len() > 1 {
        shapes.push(egui::Shape::line(line, stroke));
    }
    shapes.insert(0, egui::Shape::mesh(mesh));
    egui::Shape::Vec(shapes)
}
