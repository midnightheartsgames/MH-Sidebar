use crate::{settings_files, sidebar::MetricTarget, theme};
use eframe::egui::{self, Color32, RichText, Stroke, vec2};
use mh_sidebar::{
    analytics::{MetricDescriptor, Stats, csv_all, csv_series},
    history::{Histories, metric_key},
    model::Snapshot,
};
use std::time::{Duration, SystemTime};

#[derive(Default)]
pub struct AnalyticsWindow {
    target: Option<MetricTarget>,
    status: Option<String>,
}
pub struct AnalyticsView<'a> {
    pub snapshot: &'a Snapshot,
    pub histories: &'a Histories,
    pub now: f64,
    pub seconds: u64,
    pub origin: SystemTime,
    pub active_path: &'a std::path::Path,
}
impl AnalyticsWindow {
    #[cfg(debug_assertions)]
    pub fn is_open(&self) -> bool {
        self.target.is_some()
    }
    pub fn open(&mut self, target: MetricTarget) {
        self.target = Some(target);
        self.status = None;
    }
    pub fn show(&mut self, ctx: &egui::Context, view: AnalyticsView<'_>) {
        let AnalyticsView {
            snapshot,
            histories,
            now,
            seconds,
            origin,
            active_path,
        } = view;
        let Some(target) = self.target.clone() else {
            return;
        };
        let builder = egui::ViewportBuilder::default()
            .with_title(format!("MH Sidebar — {} · {}", target.device, target.label))
            .with_inner_size([860., 700.])
            .with_min_inner_size([650., 480.]);
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("analytics"),
            builder,
            |ui, _| {
                if ui.ctx().input(|i| i.viewport().close_requested()) {
                    self.target = None;
                    return;
                }
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(theme::BG).inner_margin(24))
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(theme::heading(&target.label, 27.));
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui.button("Закрыть").clicked() {
                                                self.target = None;
                                            }
                                        },
                                    );
                                });
                                ui.label(
                                    RichText::new(format!(
                                        "{} · {}",
                                        target.block.title(),
                                        target.device
                                    ))
                                    .color(theme::MUTED),
                                );
                                let section = snapshot.sections.iter().find(|s| {
                                    s.id == target.block && s.device_id == target.device_id
                                });
                                let row = section
                                    .and_then(|s| s.rows.iter().find(|r| r.key == target.metric));
                                ui.add_space(10.);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(row.map(|r| r.text.as_str()).unwrap_or("—"))
                                            .size(30.)
                                            .color(theme::ACCENT),
                                    );
                                    if let Some(r) = row {
                                        let age = r
                                            .sampled_at
                                            .map(|at| format!("{} с назад", at.elapsed().as_secs()))
                                            .unwrap_or_else(|| "ещё нет замера".into());
                                        ui.label(
                                            RichText::new(if r.stale {
                                                format!("Источник недоступен · {age}")
                                            } else {
                                                format!("Последний замер: {age}")
                                            })
                                            .color(theme::MUTED),
                                        );
                                    }
                                });
                                let source_state = snapshot
                                    .sources
                                    .iter()
                                    .filter(|s| s.source.block() == target.block)
                                    .map(|s| {
                                        let freshness = if s.stale {
                                            "нет свежих данных"
                                        } else {
                                            "работает"
                                        };
                                        format!("{}: {freshness}", s.source.title())
                                    })
                                    .collect::<Vec<_>>()
                                    .join(" · ");
                                if !source_state.is_empty() {
                                    ui.label(
                                        RichText::new(source_state).small().color(theme::MUTED),
                                    );
                                }
                                let window = Duration::from_secs(seconds);
                                let history = histories.get(&metric_key(
                                    target.block,
                                    &target.device_id,
                                    &target.metric,
                                ));
                                ui.add_space(12.);
                                egui::Frame::new()
                                    .fill(theme::CARD)
                                    .corner_radius(8)
                                    .inner_margin(12)
                                    .show(ui, |ui| {
                                        let width = ui.available_width();
                                        let (rect, response) = ui.allocate_exact_size(
                                            vec2(width, 215.),
                                            egui::Sense::hover(),
                                        );
                                        if let Some(history) = history {
                                            draw_graph(
                                                ui,
                                                rect,
                                                history.points(now, window),
                                                now,
                                                seconds,
                                                &target.unit,
                                                response,
                                            );
                                        } else {
                                            ui.painter().text(
                                                rect.center(),
                                                egui::Align2::CENTER_CENTER,
                                                "Пока нет истории",
                                                egui::FontId::proportional(20.),
                                                theme::MUTED,
                                            );
                                        }
                                    });
                                ui.label(
                            RichText::new(format!(
                                "Последние {seconds} с · пропуски замеров показаны разрывами"
                            ))
                            .small()
                            .color(theme::MUTED),
                        );
                                ui.add_space(12.);
                                if let Some(stats) =
                                    history.and_then(|h| Stats::from_history(h, now, window))
                                {
                                    ui.horizontal(|ui| {
                                        for (title, value) in [
                                            ("Минимум", stats.min),
                                            ("Среднее", stats.mean),
                                            ("Максимум", stats.max),
                                        ] {
                                            egui::Frame::new()
                                                .fill(theme::CARD)
                                                .inner_margin(10)
                                                .corner_radius(6)
                                                .show(ui, |ui| {
                                                    ui.set_min_width(145.);
                                                    ui.label(
                                                        RichText::new(title).color(theme::MUTED),
                                                    );
                                                    ui.label(
                                                        RichText::new(format!(
                                                            "{value:.1} {}",
                                                            target.unit
                                                        ))
                                                        .size(22.)
                                                        .color(theme::TEXT),
                                                    );
                                                });
                                        }
                                    });
                                    ui.label(
                                        RichText::new(format!(
                                            "{} замеров · пик {:.0} с назад",
                                            stats.count,
                                            (now - stats.max_at).max(0.)
                                        ))
                                        .small()
                                        .color(theme::MUTED),
                                    );
                                } else {
                                    ui.label("В выбранном окне нет числовых замеров");
                                }
                                ui.add_space(12.);
                                ui.horizontal(|ui| {
                                    if ui.button("Экспорт этого показателя в CSV…").clicked()
                                    {
                                        let data = history.map(|h| {
                                            csv_series(
                                                h,
                                                now,
                                                window,
                                                origin,
                                                MetricDescriptor {
                                                    block: &format!("{:?}", target.block),
                                                    device_id: &target.device_id,
                                                    device: &target.device,
                                                    metric: &target.metric,
                                                    label: &target.label,
                                                    unit: &target.unit,
                                                },
                                            )
                                        });
                                        self.status = Some(write_csv(data, active_path));
                                    }
                                    if ui.button("Экспорт всей истории в CSV…").clicked()
                                    {
                                        self.status = Some(write_csv(
                                            Some(csv_all(histories, now, window, origin)),
                                            active_path,
                                        ));
                                    }
                                });
                                if let Some(status) = &self.status {
                                    ui.label(RichText::new(status).small().color(theme::WARN));
                                }
                            });
                    });
            },
        );
    }
}

fn write_csv(data: Option<String>, active_path: &std::path::Path) -> String {
    let Some(data) = data else {
        return "Нет истории для экспорта".into();
    };
    match settings_files::save_csv_path(active_path) {
        Ok(Some(path)) => match std::fs::write(&path, data) {
            Ok(()) => format!("История сохранена: {}", path.display()),
            Err(e) => format!("Не удалось сохранить историю: {e}"),
        },
        Ok(None) => "Экспорт отменён".into(),
        Err(e) => e,
    }
}

fn draw_graph(
    ui: &egui::Ui,
    rect: egui::Rect,
    points: Vec<(f64, Option<f64>)>,
    now: f64,
    seconds: u64,
    unit: &str,
    response: egui::Response,
) {
    let valid: Vec<_> = points
        .iter()
        .filter_map(|(t, v)| v.map(|v| (*t, v)))
        .collect();
    if valid.is_empty() {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Нет числовых замеров",
            egui::FontId::proportional(20.),
            theme::MUTED,
        );
        return;
    }
    let max = if unit == "%" {
        100.
    } else {
        valid.iter().map(|(_, v)| *v).fold(1., f64::max) * 1.1
    };
    let plot = rect.shrink2(vec2(10., 10.));
    for i in 0..=4 {
        let y = plot.bottom() - plot.height() * i as f32 / 4.;
        ui.painter()
            .hline(plot.x_range(), y, Stroke::new(1., Color32::from_gray(55)));
        ui.painter().text(
            egui::pos2(plot.right() - 5., y - 3.),
            egui::Align2::RIGHT_BOTTOM,
            format!("{:.0}", max * i as f64 / 4.),
            egui::FontId::proportional(12.),
            theme::MUTED,
        );
    }
    let position = |(t, v): (f64, f64)| {
        egui::pos2(
            plot.left()
                + ((t - (now - seconds as f64)) / seconds as f64).clamp(0., 1.) as f32
                    * plot.width(),
            plot.bottom() - (v / max).clamp(0., 1.) as f32 * plot.height(),
        )
    };
    let mut line = Vec::new();
    for (t, v) in points {
        if let Some(v) = v {
            line.push(position((t, v)));
        } else {
            if line.len() > 1 {
                ui.painter().add(egui::Shape::line(
                    std::mem::take(&mut line),
                    Stroke::new(2.5, theme::ACCENT),
                ));
            }
            line.clear();
        }
    }
    if line.len() > 1 {
        ui.painter()
            .add(egui::Shape::line(line, Stroke::new(2.5, theme::ACCENT)));
    }
    if let Some(hover) = response.hover_pos() {
        let nearest = valid.iter().min_by(|a, b| {
            (position(**a).x - hover.x)
                .abs()
                .total_cmp(&(position(**b).x - hover.x).abs())
        });
        if let Some((t, v)) = nearest {
            let p = position((*t, *v));
            ui.painter().circle_filled(p, 5., theme::ACCENT);
            response.on_hover_text(format!("{v:.1} {unit} · {:.0} с назад", (now - t).max(0.)));
        }
    }
}
