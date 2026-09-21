use crate::{
    sidebar::{self, Histories},
    theme,
};
use eframe::egui::{self, Color32, RichText, vec2};
use mh_sidebar::{
    config::{Settings, Side},
    model::{Block, DriverStatus, Snapshot},
    platform::Monitor,
};

#[derive(Clone, Copy, PartialEq)]
enum Page {
    General,
    Metrics,
    Device(Block),
    Appearance,
    Alerts,
    Hotkeys,
    About,
}
impl Page {
    fn title(self) -> &'static str {
        match self {
            Self::General => "Общие",
            Self::Metrics => "Показатели",
            Self::Device(b) => b.title(),
            Self::Appearance => "Внешний вид",
            Self::Alerts => "Оповещения",
            Self::Hotkeys => "Горячие клавиши",
            Self::About => "О программе",
        }
    }
}

/// Requests the app to fix a missing CPU sensor driver.
#[derive(Clone, Copy)]
pub enum DriverAction {
    Install,
    OpenSite,
    RestartElevated,
}

pub struct SettingsWindow {
    pub open: bool,
    pub draft: Settings,
    page: Page,
    focus: bool,
    cover: Option<egui::TextureHandle>,
    pub status: Option<String>,
    pub first_run: bool,
    pub action: Option<DriverAction>,
}
impl SettingsWindow {
    pub fn new(settings: &Settings, first_run: bool) -> Self {
        Self {
            open: first_run,
            draft: settings.clone(),
            page: Page::General,
            focus: true,
            cover: None,
            status: None,
            first_run,
            action: None,
        }
    }
    pub fn open(&mut self, settings: &Settings) {
        if !self.open {
            self.draft = settings.clone();
        }
        self.open = true;
        self.focus = true;
    }
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        snapshot: &Snapshot,
        histories: &Histories,
        now: f64,
        monitors: &[Monitor],
    ) -> Option<Settings> {
        if !self.open {
            return None;
        }
        let mut applied = None;
        let builder = egui::ViewportBuilder::default()
            .with_title("MH Sidebar — настройки")
            .with_inner_size([1260., 840.])
            .with_min_inner_size([1000., 660.]);
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("settings"),builder,|ui,_class|{
            if std::mem::take(&mut self.focus){ui.ctx().send_viewport_cmd(egui::ViewportCommand::Focus);}
            if ui.ctx().input(|i|i.viewport().close_requested()){self.open=false;}
            egui::Panel::bottom("footer").exact_size(66.).resizable(false)
                .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(16)).show(ui,|ui|{
                    ui.horizontal(|ui|{
                        ui.allocate_ui_with_layout(vec2((ui.available_width()-320.).max(0.),34.),egui::Layout::left_to_right(egui::Align::Center),|ui|{
                            if let Some(status)=&self.status {ui.add(egui::Label::new(RichText::new(status).small().color(theme::WARN)).truncate()).on_hover_text(status);}
                            else{ui.label(RichText::new("MH SIDEBAR   /   0.1.0").small().color(theme::MUTED));}
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{
                            if ui.add_sized([140.,34.],egui::Button::new("Отмена")).clicked(){self.open=false;}
                            if ui.add_sized([150.,34.],egui::Button::new(if self.first_run{"Готово"}else{"Применить"}).fill(Color32::from_rgb(0x25,0x72,0x7B))).clicked(){self.draft.normalize();applied=Some(self.draft.clone());}
                        });
                    });
                });
            egui::Panel::left("navigation").exact_size(208.).resizable(false).frame(egui::Frame::new().fill(theme::PANEL).inner_margin(14)).show(ui,|ui|{
                ui.add_space(4.);ui.label(RichText::new("MH SIDEBAR").font(theme::bold(19.)).color(theme::ACCENT).extra_letter_spacing(1.));ui.add_space(20.);
                let pages=[Page::General,Page::Metrics,Page::Device(Block::Cpu),Page::Device(Block::Gpu),Page::Device(Block::Memory),Page::Device(Block::Disks),Page::Device(Block::Network),Page::Appearance,Page::Alerts,Page::Hotkeys,Page::About];
                for page in pages {
                    let selected=self.page==page;
                    let (rect,response)=ui.allocate_exact_size(vec2(ui.available_width(),40.),egui::Sense::click());
                    if selected||response.hovered(){ui.painter().rect_filled(rect,5,theme::CARD);}
                    if selected{ui.painter().rect_filled(egui::Rect::from_min_size(rect.min,vec2(3.,rect.height())),2,theme::ACCENT);}
                    ui.painter().text(rect.left_center()+vec2(16.,0.),egui::Align2::LEFT_CENTER,page.title(),egui::FontId::proportional(18.),if selected{theme::TEXT}else{theme::MUTED});
                    if response.clicked(){self.page=page;}
                    ui.add_space(3.);
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT),|ui|{
                    if ui.add_sized([ui.available_width(),32.],egui::Button::new("Сбросить настройки")).clicked(){self.draft=Settings::default();self.status=Some("Стандартные значения показаны в предпросмотре. Нажмите «Применить», чтобы сохранить.".into());}
                });
            });
            egui::Panel::right("preview").exact_size(310.).resizable(false).frame(egui::Frame::new().fill(theme::PANEL).inner_margin(18)).show(ui,|ui|{
                ui.label(theme::heading("Предпросмотр панели",20.));
                ui.label(RichText::new("Живые показатели вашего ПК").small().color(theme::MUTED));ui.add_space(12.);
                egui::ScrollArea::vertical().id_salt("preview-scroll").auto_shrink([false,false]).show(ui,|ui|{
                    let [r,g,b]=self.draft.background;
                    egui::Frame::new().fill(Color32::from_rgb(r,g,b)).corner_radius(8).inner_margin(12).show(ui,|ui|{
                        ui.set_width(ui.available_width());sidebar::show(ui,snapshot,&self.draft,histories,now,false);
                    });
                });
            });
            egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::BG).inner_margin(24)).show(ui,|ui|{
                egui::ScrollArea::vertical().id_salt(self.page.title()).auto_shrink([false,false]).show(ui,|ui|{
                    ui.set_width(ui.available_width()-6.);ui.label(theme::heading(self.page.title(),28.));ui.add_space(14.);
                    match self.page {
                        Page::General=>self.general(ui,monitors), Page::Metrics=>self.metrics(ui),
                        Page::Device(b)=>self.device(ui,b,snapshot),Page::Appearance=>self.appearance(ui),
                        Page::Alerts=>self.alerts(ui),Page::Hotkeys=>self.hotkeys(ui),Page::About=>self.about(ui),
                    }
                });
            });
        });
        applied
    }
    fn general(&mut self, ui: &mut egui::Ui, monitors: &[Monitor]) {
        if self.first_run {
            theme::card(ui, "Добро пожаловать", |ui| {
                ui.label("Выберите монитор и сторону панели. Справа показаны реальные показатели. «Готово» сохранит выбранное расположение.");
            });
        }
        let s = &mut self.draft;
        theme::card(ui, "Основные параметры", |ui| {
            theme::toggle(ui, "Показывать панель", &mut s.visible);
            theme::toggle(ui, "Поверх всех окон", &mut s.always_on_top);
            theme::toggle(ui, "Зарезервировать пространство", &mut s.reserve_space);
            ui.label(
                RichText::new("При резервировании развёрнутые окна оставляют место для панели.")
                    .small()
                    .color(theme::MUTED),
            );
            theme::toggle(ui, "Пропускать клики сквозь панель", &mut s.locked);
            theme::toggle(ui, "Запускать при входе в Windows", &mut s.autostart);
            theme::row(ui, "Интервал обновления", |ui| {
                egui::ComboBox::from_id_salt("interval")
                    .selected_text(format!("{} мс", s.interval_ms))
                    .show_ui(ui, |ui| {
                        for n in [500, 1000, 2000, 5000] {
                            ui.selectable_value(&mut s.interval_ms, n, format!("{n} мс"));
                        }
                    });
            });
        });
        theme::card(ui, "Расположение", |ui| {
            let selected = monitors
                .iter()
                .find(|m| m.id == s.monitor_id)
                .map(|m| m.name.clone())
                .unwrap_or_else(|| "Авто · второй монитор".into());
            egui::ComboBox::from_id_salt("monitor")
                .width(ui.available_width())
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut s.monitor_id, String::new(), "Авто · второй монитор");
                    for monitor in monitors {
                        ui.selectable_value(
                            &mut s.monitor_id,
                            monitor.id.clone(),
                            format!(
                                "{} · {}×{} · {}%",
                                monitor.name,
                                monitor.rect.right - monitor.rect.left,
                                monitor.rect.bottom - monitor.rect.top,
                                (monitor.scale * 100.) as u32
                            ),
                        );
                    }
                });
            ui.add_space(8.);
            theme::row(ui, "Сторона", |ui| {
                ui.selectable_value(&mut s.side, Side::Left, "Слева");
                ui.selectable_value(&mut s.side, Side::Right, "Справа");
            });
            theme::row(ui, "Ширина панели", |ui| {
                ui.add(egui::Slider::new(&mut s.width, 220. ..=480.).suffix(" px"));
            });
        });
    }
    fn metrics(&mut self, ui: &mut egui::Ui) {
        theme::card(
            ui,
            "Порядок и видимость блоков",
            |ui| {
                ui.label(
                    RichText::new("Стрелки меняют порядок блоков в панели.")
                        .small()
                        .color(theme::MUTED),
                );
                ui.add_space(8.);
                let mut movement = None;
                let len = self.draft.blocks.len();
                for (i, b) in self.draft.blocks.iter_mut().enumerate() {
                    ui.push_id(i, |ui| {
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut b.enabled, b.id.title());
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add_enabled(i + 1 < len, egui::Button::new("↓"))
                                        .clicked()
                                    {
                                        movement = Some((i, i + 1));
                                    }
                                    if ui.add_enabled(i > 0, egui::Button::new("↑")).clicked() {
                                        movement = Some((i, i - 1));
                                    }
                                },
                            );
                        });
                        ui.separator();
                    });
                }
                if let Some((a, b)) = movement {
                    self.draft.blocks.swap(a, b);
                }
            },
        );
        theme::card(ui, "Отображение", |ui| {
            theme::toggle(
                ui,
                "Скрывать недоступные показатели",
                &mut self.draft.hide_unavailable,
            );
            theme::row(ui, "История графиков", |ui| {
                egui::ComboBox::from_id_salt("history")
                    .selected_text(format!("{} с", self.draft.graph_seconds))
                    .show_ui(ui, |ui| {
                        for n in [30, 60, 120, 300] {
                            ui.selectable_value(&mut self.draft.graph_seconds, n, format!("{n} с"));
                        }
                    });
            });
        });
    }
    fn device(&mut self, ui: &mut egui::Ui, id: Block, snapshot: &Snapshot) {
        if id == Block::Cpu {
            theme::card(ui, "Процессор", |ui| {
                theme::toggle(
                    ui,
                    "Показывать загрузку потоков",
                    &mut self.draft.show_cores,
                );
                ui.label(RichText::new("Температура и мощность CPU читаются через драйвер PawnIO. Его состояние — в «Состоянии датчиков» ниже.").small().color(theme::MUTED));
            });
        }
        let Some(block) = self.draft.blocks.iter_mut().find(|b| b.id == id) else {
            return;
        };
        theme::card(ui, "Настройки блока", |ui| {
            theme::toggle(ui, "Показывать блок", &mut block.enabled);
            theme::toggle(ui, "Название оборудования", &mut block.show_name);
            theme::toggle(ui, "График", &mut block.graph);
            let mut rows = Vec::new();
            for section in snapshot.sections.iter().filter(|s| s.id == id) {
                for r in &section.rows {
                    if !r.key.starts_with("core_") && !rows.iter().any(|(key, _)| key == &r.key) {
                        rows.push((r.key.clone(), r.label.clone()));
                    }
                }
            }
            theme::row(ui, "Показатель графика", |ui| {
                egui::ComboBox::from_id_salt("graph-metric")
                    .selected_text(
                        rows.iter()
                            .find(|(k, _)| k == &block.graph_metric)
                            .map(|(_, l)| l.as_str())
                            .unwrap_or("Загрузка"),
                    )
                    .show_ui(ui, |ui| {
                        for (key, label) in &rows {
                            ui.selectable_value(&mut block.graph_metric, key.clone(), label);
                        }
                    });
            });
            ui.add_space(8.);
            for (key, label) in rows {
                let mut checked = block.shows(&key);
                if ui.checkbox(&mut checked, label).changed() {
                    block.set_row(&key, checked);
                }
            }
        });
        theme::card(ui, "Устройства", |ui| {
            let mut all = block.devices.is_empty();
            if ui
                .checkbox(&mut all, "Все обнаруженные устройства")
                .changed()
            {
                if all {
                    block.devices.clear();
                } else {
                    block.devices = snapshot
                        .sections
                        .iter()
                        .filter(|s| s.id == id)
                        .take(1)
                        .map(|s| s.device.clone())
                        .collect();
                }
            }
            for section in snapshot.sections.iter().filter(|s| s.id == id) {
                let mut selected = all || block.devices.contains(&section.device);
                if ui
                    .add_enabled(!all, egui::Checkbox::new(&mut selected, &section.device))
                    .changed()
                {
                    if selected {
                        block.devices.push(section.device.clone());
                    } else if block.devices.len() > 1 {
                        block.devices.retain(|d| d != &section.device);
                    }
                }
            }
            ui.label(
                RichText::new("Выберите хотя бы одно устройство или отключите блок целиком.")
                    .small()
                    .color(theme::MUTED),
            );
        });
        theme::card(ui, "Состояние датчиков", |ui| {
            for section in snapshot.sections.iter().filter(|s| s.id == id) {
                ui.label(theme::heading(&section.device, 17.));
                for row in section.rows.iter().filter(|r| !r.key.starts_with("core_")) {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(&row.label);
                        ui.label(
                            RichText::new(row.reason.as_deref().unwrap_or(&row.text))
                                .small()
                                .color(if row.reason.is_some() {
                                    theme::WARN
                                } else {
                                    theme::MUTED
                                }),
                        );
                    });
                }
            }
            if id == Block::Cpu {
                self.driver(ui, snapshot.cpu_driver);
            }
        });
    }
    /// Status of the PawnIO driver with the action that fixes it.
    fn driver(&mut self, ui: &mut egui::Ui, status: DriverStatus) {
        let text = match status {
            DriverStatus::Unknown | DriverStatus::Ready => return,
            DriverStatus::NotInstalled => {
                "Для температуры и мощности CPU нужен бесплатный драйвер PawnIO (pawnio.eu). Установка потребует подтверждения администратора, затем MH Sidebar нужно запустить от имени администратора."
            }
            DriverStatus::NeedsAdmin if mh_sidebar::platform::is_elevated() => {
                "Драйвер PawnIO не дал доступ даже администратору. Переустановите PawnIO."
            }
            DriverStatus::NeedsAdmin => {
                "Драйвер PawnIO установлен, но открыть его может только администратор."
            }
            DriverStatus::Unsupported => {
                "Для этого процессора у PawnIO нет модуля: поддерживаются AMD Ryzen (Zen 1–5) и Intel с цифровым датчиком."
            }
            DriverStatus::Failed => "Драйвер PawnIO отклонил запрос. Попробуйте обновить PawnIO.",
        };
        ui.add_space(10.);
        ui.label(RichText::new(text).small().color(theme::WARN));
        ui.add_space(6.);
        let buttons: &[(&str, DriverAction)] = match status {
            DriverStatus::NotInstalled | DriverStatus::Failed => &[
                ("Установить PawnIO", DriverAction::Install),
                ("Открыть pawnio.eu", DriverAction::OpenSite),
            ],
            DriverStatus::NeedsAdmin if !mh_sidebar::platform::is_elevated() => &[(
                "Перезапустить от имени администратора",
                DriverAction::RestartElevated,
            )],
            _ => &[],
        };
        ui.horizontal(|ui| {
            for (label, action) in buttons {
                if ui.button(*label).clicked() {
                    self.action = Some(*action);
                }
            }
        });
    }
    fn appearance(&mut self, ui: &mut egui::Ui) {
        let s = &mut self.draft;
        theme::card(ui, "Оформление панели", |ui| {
            theme::row(ui, "Непрозрачность фона", |ui| {
                ui.add(
                    egui::Slider::new(&mut s.opacity, 0.1..=1.)
                        .custom_formatter(|n, _| format!("{}%", (n * 100.).round())),
                );
            });
            theme::row(ui, "Размер шрифта", |ui| {
                ui.add(egui::Slider::new(&mut s.font_size, 12. ..=26.).suffix(" px"));
            });
            theme::row(ui, "Цвет фона", |ui| {
                ui.color_edit_button_srgb(&mut s.background);
            });
            theme::row(ui, "Цвет акцента", |ui| {
                ui.color_edit_button_srgb(&mut s.accent);
            });
            theme::toggle(ui, "Заголовок с кнопками", &mut s.show_header);
        });
        theme::card(ui, "Часы", |ui| {
            theme::toggle(ui, "24-часовой формат", &mut s.clock_24h);
            theme::toggle(ui, "Показывать секунды", &mut s.show_seconds);
            theme::toggle(ui, "Показывать дату", &mut s.show_date);
        });
    }
    fn alerts(&mut self, ui: &mut egui::Ui) {
        theme::card(ui, "Температура GPU", |ui| {
            theme::toggle(ui, "Выделять высокую температуру", &mut self.draft.alerts);
            theme::row(ui, "Предупреждение", |ui| {
                ui.add(
                    egui::DragValue::new(&mut self.draft.warning_temperature)
                        .range(30. ..=110.)
                        .suffix(" °C"),
                );
            });
            theme::row(ui, "Критическая температура", |ui| {
                ui.add(
                    egui::DragValue::new(&mut self.draft.critical_temperature)
                        .range(31. ..=120.)
                        .suffix(" °C"),
                );
            });
            ui.label(RichText::new("Предупреждение").color(theme::WARN));
            ui.label(RichText::new("Критическое значение").color(theme::CRITICAL));
            ui.label(RichText::new("Меняется цвет значения на панели. Системные уведомления и звуки не используются.").small().color(theme::MUTED));
        });
    }
    fn hotkeys(&mut self, ui: &mut egui::Ui) {
        theme::card(ui, "Глобальные сочетания", |ui| {
            for (label, value) in [
                ("Показать / скрыть", &mut self.draft.hotkeys.visibility),
                ("Пропускать клики", &mut self.draft.hotkeys.lock),
                ("Открыть настройки", &mut self.draft.hotkeys.settings),
            ] {
                ui.label(label);
                ui.add(egui::TextEdit::singleline(value).desired_width(ui.available_width()));
                ui.add_space(8.);
            }
            ui.label(RichText::new("Например: Ctrl+Alt+F11. Пустое поле отключает сочетание. Если оно занято другой программой, здесь появится сообщение после применения.").small().color(theme::MUTED));
        });
    }
    fn about(&mut self, ui: &mut egui::Ui) {
        if self.cover.is_none()
            && let Ok(img) = image::load_from_memory(include_bytes!("../assets/mh-sidebar.png"))
        {
            let img = img.to_rgba8();
            let size = [img.width() as usize, img.height() as usize];
            self.cover = Some(ui.ctx().load_texture(
                "mh-sidebar-cover",
                egui::ColorImage::from_rgba_unmultiplied(size, img.as_raw()),
                egui::TextureOptions::LINEAR,
            ));
        }
        if let Some(cover) = &self.cover {
            ui.add(
                egui::Image::new(cover)
                    .fit_to_exact_size(vec2(ui.available_width(), ui.available_width()))
                    .corner_radius(8),
            );
            ui.add_space(14.);
        }
        theme::card(ui, "MH Sidebar 0.1.0", |ui| {
            ui.label("Мониторинг системы для второго экрана.");
            ui.label(RichText::new("Rust · egui · Windows").color(theme::ACCENT));
            ui.label("Оформление и часть системных модулей — MH Monitoring, midnightheartsgames. Шрифт Cuprum.");
            ui.label("Температура и мощность CPU — через драйвер PawnIO (ставится отдельно, нужны права администратора); без него остальное работает без администратора. NVIDIA: NVML; другие GPU: доступные показатели WDDM. Данные не отправляются в сеть.");
            ui.label(
                RichText::new(mh_sidebar::config::default_path().display().to_string())
                    .small()
                    .color(theme::MUTED),
            );
        });
    }
}
