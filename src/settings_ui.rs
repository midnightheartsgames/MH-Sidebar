use crate::{
    sidebar::{self, Histories},
    theme,
};
use eframe::egui::{self, Color32, RichText, vec2};
#[cfg(windows)]
use mh_sidebar::model::DriverStatus;
use mh_sidebar::{
    config::{Density, Preset, Settings, Side},
    model::{Block, Snapshot},
    platform::Monitor,
};

#[derive(Clone, Copy, PartialEq)]
enum Page {
    General,
    Profiles,
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
            Self::Profiles => "Профили",
            Self::Metrics => "Показатели",
            Self::Device(b) => b.title(),
            Self::Appearance => "Внешний вид",
            Self::Alerts => "Оповещения",
            Self::Hotkeys => "Горячие клавиши",
            Self::About => "О программе",
        }
    }
}
#[derive(Clone, Copy)]
#[cfg(windows)]
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
    #[cfg(windows)]
    pub action: Option<DriverAction>,
    profile_name: String,
    pub recording: Option<usize>,
}
impl SettingsWindow {
    pub fn new(settings: &Settings, first_run: bool) -> Self {
        let page = Page::General;
        #[cfg(debug_assertions)]
        let page = match std::env::var("MH_SIDEBAR_SETTINGS_PAGE").as_deref() {
            Ok("profiles") => Page::Profiles,
            Ok("hotkeys") => Page::Hotkeys,
            Ok("metrics") => Page::Metrics,
            Ok("appearance") => Page::Appearance,
            Ok("alerts") => Page::Alerts,
            _ => page,
        };
        Self {
            open: first_run,
            draft: settings.clone(),
            page,
            focus: true,
            cover: None,
            status: None,
            first_run,
            #[cfg(windows)]
            action: None,
            profile_name: String::new(),
            recording: None,
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
        active_path: &std::path::Path,
    ) -> Option<Settings> {
        if !self.open {
            self.recording = None;
            return None;
        }
        let mut applied = None;
        let builder = egui::ViewportBuilder::default()
            .with_title("MH Sidebar — настройки")
            .with_inner_size([1260., 840.])
            .with_min_inner_size([1000., 660.]);
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("settings"),builder,|ui,_class|{
            self.capture_hotkey(ui.ctx());
            if std::mem::take(&mut self.focus){ui.ctx().send_viewport_cmd(egui::ViewportCommand::Focus);}
            if ui.ctx().input(|i|i.viewport().close_requested()){self.open=false;}
            egui::Panel::bottom("footer").exact_size(66.).resizable(false)
                .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(16)).show(ui,|ui|{
                    ui.horizontal(|ui|{
                        ui.allocate_ui_with_layout(vec2((ui.available_width()-320.).max(0.),34.),egui::Layout::left_to_right(egui::Align::Center),|ui|{
                            if let Some(status)=&self.status {ui.add(egui::Label::new(RichText::new(status).small().color(theme::WARN)).truncate()).on_hover_text(status);}
                            else{ui.label(RichText::new(concat!("MH SIDEBAR   /   ", env!("CARGO_PKG_VERSION"))).small().color(theme::MUTED));}
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{
                            if ui.add_sized([140.,34.],egui::Button::new("Отмена")).clicked(){self.open=false;}
                            if ui.add_sized([150.,34.],egui::Button::new(if self.first_run{"Готово"}else{"Применить"}).fill(Color32::from_rgb(0x25,0x72,0x7B))).clicked(){self.draft.normalize();applied=Some(self.draft.clone());}
                        });
                    });
                });
            egui::Panel::left("navigation").exact_size(208.).resizable(false).frame(egui::Frame::new().fill(theme::PANEL).inner_margin(14)).show(ui,|ui|{
                ui.add_space(4.);ui.label(RichText::new("MH SIDEBAR").font(theme::bold(19.)).color(theme::ACCENT).extra_letter_spacing(1.));ui.add_space(20.);
                let pages=[Page::General,Page::Profiles,Page::Metrics,Page::Device(Block::Cpu),Page::Device(Block::Gpu),Page::Device(Block::Memory),Page::Device(Block::Disks),Page::Device(Block::Network),Page::Appearance,Page::Alerts,Page::Hotkeys,Page::About];
                for page in pages {
                    let selected=self.page==page;
                    let (rect,response)=ui.allocate_exact_size(vec2(ui.available_width(),34.),egui::Sense::click());
                    if selected||response.hovered(){ui.painter().rect_filled(rect,5,theme::CARD);}
                    if selected{ui.painter().rect_filled(egui::Rect::from_min_size(rect.min,vec2(3.,rect.height())),2,theme::ACCENT);}
                    ui.painter().text(rect.left_center()+vec2(16.,0.),egui::Align2::LEFT_CENTER,page.title(),egui::FontId::proportional(18.),if selected{theme::TEXT}else{theme::MUTED});
                    if response.clicked(){self.page=page;self.recording=None;}
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
                        Page::Profiles=>self.profiles(ui,active_path),
                        Page::Device(b)=>self.device(ui,b,snapshot),Page::Appearance=>self.appearance(ui),
                        Page::Alerts=>self.alerts(ui,snapshot),Page::Hotkeys=>self.hotkeys(ui),Page::About=>self.about(ui),
                    }
                });
            });
        });
        if !self.open || applied.is_some() {
            self.recording = None;
        }
        applied
    }
    fn profiles(&mut self, ui: &mut egui::Ui, active_path: &std::path::Path) {
        theme::card(ui, "Готовые профили", |ui| {
            ui.label(
                "Меняют оформление и блоки. Монитор, автозапуск и горячие клавиши сохраняются.",
            );
            ui.horizontal_wrapped(|ui| {
                for preset in [Preset::Minimal, Preset::Gaming, Preset::Work] {
                    if ui.button(preset.title()).clicked() {
                        self.draft.apply_preset(preset);
                    }
                }
            });
        });
        theme::card(ui, "Мои профили", |ui| {
            ui.label("Сохраните оформление и выбранные устройства из предпросмотра.");
            ui.add(
                egui::TextEdit::singleline(&mut self.profile_name)
                    .hint_text("Название профиля")
                    .char_limit(64),
            );
            let exists = self
                .draft
                .profiles
                .iter()
                .any(|p| p.name == self.profile_name.trim());
            if ui
                .button(if exists {
                    "Обновить профиль"
                } else {
                    "Сохранить профиль"
                })
                .clicked()
            {
                self.status = Some(match self.draft.save_profile(&self.profile_name) {
                    Ok(()) => {
                        "Профиль добавлен в черновик. Нажмите «Применить» для сохранения.".into()
                    }
                    Err(e) => e,
                });
            }
            let mut apply = None;
            let mut remove = None;
            for (i, profile) in self.draft.profiles.iter().enumerate() {
                ui.push_id(i, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_sized(
                            [(ui.available_width() - 175.).max(60.), 26.],
                            egui::Label::new(&profile.name).truncate(),
                        )
                        .on_hover_text(&profile.name);
                        if ui.button("Выбрать").clicked() {
                            apply = Some(i);
                        }
                        if ui.button("Удалить").clicked() {
                            remove = Some(i);
                        }
                    });
                });
            }
            if let Some(i) = apply {
                let _ = self.draft.apply_profile(i);
            }
            if let Some(i) = remove {
                self.draft.profiles.remove(i);
            }
        });
        theme::card(ui, "Перенос настроек", |ui| {
            ui.label("Импорт заменяет весь черновик, включая профили и горячие клавиши. Проверьте предпросмотр и нажмите «Применить» или «Отмена».");
            if ui.button("Импортировать JSON…").clicked() {
                match crate::settings_files::import() {
                    Ok(Some(s)) => {
                        self.draft = s;
                        self.status = Some("Настройки импортированы в черновик".into());
                    }
                    Ok(None) => {}
                    Err(e) => self.status = Some(e),
                }
            }
            if ui.button("Экспортировать черновик…").clicked() {
                match crate::settings_files::export(&self.draft, active_path) {
                    Ok(true) => self.status = Some("Черновик экспортирован".into()),
                    Ok(false) => {}
                    Err(e) => self.status = Some(e),
                }
            }
        });
    }
    fn general(&mut self, ui: &mut egui::Ui, monitors: &[Monitor]) {
        if self.first_run {
            theme::card(ui, "Добро пожаловать", |ui| {
                #[cfg(windows)]
                ui.label("Выберите монитор и сторону панели. Автозапуск с правами администратора включён по умолчанию; при нажатии «Готово» Windows запросит подтверждение UAC. Справа показаны реальные показатели.");
                #[cfg(not(windows))]
                ui.label("Выберите экран и сторону панели. Автозапуск включён по умолчанию. Справа показаны доступные системные показатели.");
            });
        }
        let s = &mut self.draft;
        theme::card(ui, "Основные параметры", |ui| {
            theme::toggle(ui, "Показывать панель", &mut s.visible);
            theme::toggle(ui, "Поверх всех окон", &mut s.always_on_top);
            #[cfg(windows)]
            {
                theme::toggle(ui, "Зарезервировать пространство", &mut s.reserve_space);
                ui.label(
                    RichText::new(
                        "При резервировании развёрнутые окна оставляют место для панели.",
                    )
                    .small()
                    .color(theme::MUTED),
                );
            }
            theme::toggle(ui, "Пропускать клики сквозь панель", &mut s.locked);
            #[cfg(windows)]
            {
                theme::toggle(
                    ui,
                    "Запускать от администратора при входе в Windows",
                    &mut s.autostart,
                );
                ui.label(RichText::new("При включении Windows один раз запросит подтверждение UAC. Затем программа запускается с повышенными правами без повторного запроса.").small().color(theme::MUTED));
            }
            #[cfg(not(windows))]
            theme::toggle(ui, "Запускать при входе в систему", &mut s.autostart);
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
                    RichText::new("Перетащите блок за ≡ или используйте стрелки.")
                        .small()
                        .color(theme::MUTED),
                );
                ui.add_space(8.);
                let mut movement = None;
                let len = self.draft.blocks.len();
                for (i, b) in self.draft.blocks.iter_mut().enumerate() {
                    ui.push_id(b.id, |ui| {
                        let row = ui.horizontal(|ui| {
                            ui.dnd_drag_source(ui.id().with("handle"), b.id, |ui| {
                                ui.label("≡");
                            });
                            ui.checkbox(&mut b.enabled, b.id.title());
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add_enabled(i + 1 < len, egui::Button::new("↓"))
                                        .clicked()
                                    {
                                        movement = Some((b.id, i + 1));
                                    }
                                    if ui.add_enabled(i > 0, egui::Button::new("↑")).clicked() {
                                        movement = Some((b.id, i - 1));
                                    }
                                },
                            );
                        });
                        if row.response.dnd_hover_payload::<Block>().is_some() {
                            ui.painter().hline(
                                row.response.rect.x_range(),
                                row.response.rect.bottom(),
                                (2., theme::ACCENT),
                            );
                        }
                        if let Some(source) = row.response.dnd_release_payload::<Block>() {
                            movement = Some((*source, i));
                        }
                        ui.separator();
                    });
                }
                if let Some((a, b)) = movement {
                    self.draft.move_block(a, b);
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
        let show_cores = self.draft.show_cores;
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
                    let key = mh_sidebar::config::BlockConfig::row_group(&r.key);
                    if !rows.iter().any(|(saved, _)| saved == key) {
                        rows.push((
                            key.to_owned(),
                            if key == "core_*" {
                                "Загрузка потоков".to_owned()
                            } else {
                                r.label.clone()
                            },
                        ));
                    }
                }
            }
            rows.sort_by_key(|(key, _)| block.row_position(key));
            theme::row(ui, "Показатель графика", |ui| {
                egui::ComboBox::from_id_salt("graph-metric")
                    .selected_text(
                        rows.iter()
                            .find(|(k, _)| k == &block.graph_metric)
                            .map(|(_, l)| l.as_str())
                            .unwrap_or("Загрузка"),
                    )
                    .show_ui(ui, |ui| {
                        for (key, label) in rows.iter().filter(|(key, _)| key != "core_*") {
                            ui.selectable_value(&mut block.graph_metric, key.clone(), label);
                        }
                    });
            });
            ui.add_space(8.);
            ui.label(
                RichText::new("Порядок показателей: перетащите за ≡ или используйте стрелки.")
                    .small()
                    .color(theme::MUTED),
            );
            let available: Vec<_> = rows.iter().map(|(key, _)| key.clone()).collect();
            let mut movement = None;
            for (index, (key, label)) in rows.iter().enumerate() {
                ui.push_id(key, |ui| {
                    let row = ui.horizontal(|ui| {
                        ui.dnd_drag_source(ui.id().with("handle"), key.clone(), |ui| {
                            ui.label("≡");
                        });
                        if key == "core_*" {
                            ui.label(if show_cores {
                                label.clone()
                            } else {
                                format!("{label} (выключено)")
                            });
                        } else {
                            let mut checked = block.shows(key);
                            if ui.checkbox(&mut checked, label).changed() {
                                block.set_row(key, checked);
                            }
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add_enabled(index + 1 < rows.len(), egui::Button::new("↓"))
                                .clicked()
                            {
                                movement = Some((key.clone(), index + 1));
                            }
                            if ui.add_enabled(index > 0, egui::Button::new("↑")).clicked() {
                                movement = Some((key.clone(), index - 1));
                            }
                        });
                    });
                    if row.response.dnd_hover_payload::<String>().is_some() {
                        ui.painter().hline(
                            row.response.rect.x_range(),
                            row.response.rect.bottom(),
                            (2., theme::ACCENT),
                        );
                    }
                    if let Some(source) = row.response.dnd_release_payload::<String>() {
                        movement = Some(((*source).clone(), index));
                    }
                });
            }
            if let Some((key, target)) = movement {
                block.move_row(&key, target, &available);
            }
        });
        theme::card(ui, "Устройства", |ui| {
            let mut all = block.all_devices();
            let available: Vec<_> = snapshot
                .sections
                .iter()
                .filter(|s| s.id == id && !s.disconnected)
                .collect();
            if ui
                .add_enabled(
                    !all || !available.is_empty(),
                    egui::Checkbox::new(&mut all, "Все обнаруженные устройства"),
                )
                .changed()
            {
                block.devices.clear();
                block.device_ids.clear();
                if !all && let Some(first) = available.first() {
                    block.device_ids.push(first.device_id.clone());
                }
            }
            for section in &available {
                let mut selected = all || block.device_ids.contains(&section.device_id);
                let response = ui
                    .push_id(&section.device_id, |ui| {
                        ui.add_enabled(!all, egui::Checkbox::new(&mut selected, &section.device))
                    })
                    .inner;
                if response.changed() {
                    if selected {
                        block.device_ids.push(section.device_id.clone());
                    } else if block.device_ids.len() + block.devices.len() > 1 {
                        block.device_ids.retain(|d| d != &section.device_id);
                    }
                }
                if section.device_id.starts_with("unstable:") {
                    ui.label(RichText::new("Постоянный ID недоступен: выбор может измениться после переподключения.").small().color(theme::WARN));
                }
            }
            let missing: Vec<_> = block
                .device_ids
                .iter()
                .filter(|saved| !available.iter().any(|s| &s.device_id == *saved))
                .cloned()
                .collect();
            for saved in missing {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Выбранное устройство отключено").color(theme::WARN))
                        .on_hover_text(&saved);
                    if ui
                        .add_enabled(
                            block.device_ids.len() + block.devices.len() > 1,
                            egui::Button::new("Убрать"),
                        )
                        .clicked()
                    {
                        block.device_ids.retain(|d| d != &saved);
                    }
                });
            }
            for legacy in block.devices.clone() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!("Ожидает сопоставления: {legacy}"))
                            .small()
                            .color(theme::WARN),
                    );
                    if ui
                        .add_enabled(
                            block.device_ids.len() + block.devices.len() > 1,
                            egui::Button::new("Убрать"),
                        )
                        .clicked()
                    {
                        block.devices.retain(|d| d != &legacy);
                    }
                });
            }
            ui.label(
                RichText::new("Выберите хотя бы одно устройство или отключите блок целиком.")
                    .small()
                    .color(theme::MUTED),
            );
        });
        theme::card(ui, "Состояние датчиков", |ui| {
            for source in snapshot.sources.iter().filter(|s| s.source.block() == id) {
                let state = if let Some(error) = &source.error {
                    error.clone()
                } else if let Some(age) = source.age {
                    format!(
                        "{} · последний замер {} с назад",
                        if source.stale {
                            "Данные устарели"
                        } else {
                            "Обновляется"
                        },
                        age.as_secs()
                    )
                } else {
                    "Ожидание первого замера".into()
                };
                ui.label(
                    RichText::new(format!("{}: {state}", source.source.title()))
                        .small()
                        .color(if source.stale {
                            theme::WARN
                        } else {
                            theme::MUTED
                        }),
                );
            }
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
                ui.label(
                    RichText::new(&snapshot.cpu_diagnostic)
                        .small()
                        .color(theme::MUTED),
                );
                #[cfg(windows)]
                self.driver(ui, snapshot.cpu_driver);
            }
        });
    }
    #[cfg(windows)]
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
            DriverStatus::Failed => {
                "Не удалось получить показания CPU. Приложение повторяет попытки автоматически; подробности указаны выше."
            }
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
            theme::row(ui, "Плотность", |ui| {
                egui::ComboBox::from_id_salt("density")
                    .selected_text(s.density.title())
                    .show_ui(ui, |ui| {
                        for density in [Density::Compact, Density::Normal, Density::Spacious] {
                            ui.selectable_value(&mut s.density, density, density.title());
                        }
                    });
            });
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
    fn alerts(&mut self, ui: &mut egui::Ui, snapshot: &Snapshot) {
        theme::toggle(ui, "Выделять высокую температуру", &mut self.draft.alerts);
        let cpu = self
            .draft
            .cpu_temperature
            .get_or_insert(mh_sidebar::config::TemperatureLimits {
                warning: self.draft.warning_temperature,
                critical: self.draft.critical_temperature,
            });
        for (title, warning, critical) in [
            ("Температура CPU", &mut cpu.warning, &mut cpu.critical),
            (
                "Температура GPU",
                &mut self.draft.warning_temperature,
                &mut self.draft.critical_temperature,
            ),
        ] {
            theme::card(ui, title, |ui| {
                theme::row(ui, "Предупреждение", |ui| {
                    ui.add(
                        egui::DragValue::new(warning)
                            .range(30. ..=110.)
                            .suffix(" °C"),
                    );
                });
                theme::row(ui, "Критическая температура", |ui| {
                    ui.add(
                        egui::DragValue::new(critical)
                            .range(31. ..=120.)
                            .suffix(" °C"),
                    );
                });
            });
        }
        ui.label(RichText::new("Предупреждение").color(theme::WARN));
        ui.label(RichText::new("Критическое значение").color(theme::CRITICAL));
        ui.label(
            RichText::new(
                "Эти пороги меняют цвет значения на панели. Правила системных уведомлений задаются ниже.",
            )
            .small()
            .color(theme::MUTED),
        );
        ui.add_space(14.);
        theme::card(ui, "Системные предупреждения", |ui| {
            theme::toggle(
                ui,
                "Показывать уведомления Windows",
                &mut self.draft.notifications_enabled,
            );
            ui.label(RichText::new("Правило сработает, когда показатель непрерывно превышает порог. Недоступные и устаревшие показания не учитываются.").small().color(theme::MUTED));
            ui.horizontal_wrapped(|ui| {
                for (label, block, metric, threshold) in [
                    ("+ Температура CPU", Block::Cpu, "temperature", 80.),
                    ("+ Температура GPU", Block::Gpu, "temperature", 80.),
                    ("+ Загрузка RAM", Block::Memory, "load", 90.),
                    ("+ Заполнение диска", Block::Disks, "load", 90.),
                ] {
                    if ui.button(label).clicked() {
                        self.draft.alert_rules.push(mh_sidebar::config::AlertRule {
                            block,
                            metric: metric.into(),
                            threshold,
                            ..Default::default()
                        });
                    }
                }
            });
        });
        let mut remove = None;
        for (i, rule) in self.draft.alert_rules.iter_mut().enumerate() {
            ui.push_id(i, |ui| {
                theme::card(ui, &format!("Правило {}", i + 1), |ui| {
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut rule.enabled, "Включено");
                        if ui.button("Удалить правило").clicked() {
                            remove = Some(i);
                        }
                    });
                    theme::row(ui, "Блок", |ui| {
                        egui::ComboBox::from_id_salt("alert-block")
                            .selected_text(rule.block.title())
                            .show_ui(ui, |ui| {
                                for block in [
                                    Block::Cpu,
                                    Block::Gpu,
                                    Block::Memory,
                                    Block::Disks,
                                    Block::Network,
                                ] {
                                    if ui
                                        .selectable_value(&mut rule.block, block, block.title())
                                        .changed()
                                    {
                                        rule.metric =
                                            mh_sidebar::analytics::default_metric(block).into();
                                        rule.device_id.clear();
                                    }
                                }
                            });
                    });
                    let mut metrics = Vec::new();
                    for section in snapshot
                        .sections
                        .iter()
                        .filter(|s| s.id == rule.block && !s.disconnected)
                    {
                        for row in &section.rows {
                            if !metrics.iter().any(|(key, _)| key == &row.key) {
                                metrics.push((row.key.clone(), row.label.clone()));
                            }
                        }
                    }
                    if !metrics.iter().any(|(key, _)| key == &rule.metric) {
                        metrics.push((rule.metric.clone(), rule.metric.clone()));
                    }
                    theme::row(ui, "Показатель", |ui| {
                        let label = metrics
                            .iter()
                            .find(|(k, _)| k == &rule.metric)
                            .map(|(_, l)| l.as_str())
                            .unwrap_or(&rule.metric);
                        egui::ComboBox::from_id_salt("alert-metric")
                            .selected_text(label)
                            .show_ui(ui, |ui| {
                                for (key, label) in &metrics {
                                    ui.selectable_value(&mut rule.metric, key.clone(), label);
                                }
                            });
                    });
                    theme::row(ui, "Устройство", |ui| {
                        let selected = if rule.device_id.is_empty() {
                            "Все устройства".to_owned()
                        } else {
                            snapshot
                                .sections
                                .iter()
                                .find(|s| s.id == rule.block && s.device_id == rule.device_id)
                                .map(|s| s.device.clone())
                                .unwrap_or_else(|| format!("Сохранённое: {}", rule.device_id))
                        };
                        egui::ComboBox::from_id_salt("alert-device")
                            .selected_text(selected)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut rule.device_id,
                                    String::new(),
                                    "Все устройства",
                                );
                                for section in snapshot
                                    .sections
                                    .iter()
                                    .filter(|s| s.id == rule.block && !s.disconnected)
                                {
                                    ui.selectable_value(
                                        &mut rule.device_id,
                                        section.device_id.clone(),
                                        &section.device,
                                    );
                                }
                            });
                    });
                    theme::row(ui, "Порог", |ui| {
                        ui.add(
                            egui::DragValue::new(&mut rule.threshold)
                                .speed(0.5)
                                .range(0. ..=1_000_000.),
                        );
                    });
                    theme::row(ui, "Длительность", |ui| {
                        let mut seconds = rule.duration_ms / 1000;
                        if ui
                            .add(
                                egui::DragValue::new(&mut seconds)
                                    .range(0..=120)
                                    .suffix(" с"),
                            )
                            .changed()
                        {
                            rule.duration_ms = seconds * 1000;
                        }
                    });
                    theme::row(
                        ui,
                        "Пауза между уведомлениями",
                        |ui| {
                            let mut seconds = rule.cooldown_ms / 1000;
                            if ui
                                .add(
                                    egui::DragValue::new(&mut seconds)
                                        .range(1..=3600)
                                        .suffix(" с"),
                                )
                                .changed()
                            {
                                rule.cooldown_ms = seconds * 1000;
                            }
                        },
                    );
                });
            });
        }
        if let Some(i) = remove {
            self.draft.alert_rules.remove(i);
        }
    }
    fn hotkeys(&mut self, ui: &mut egui::Ui) {
        theme::card(ui, "Глобальные сочетания", |ui| {
            for (index, (label, value)) in [
                ("Показать / скрыть", &mut self.draft.hotkeys.visibility),
                ("Пропускать клики", &mut self.draft.hotkeys.lock),
                ("Открыть настройки", &mut self.draft.hotkeys.settings),
            ]
            .into_iter()
            .enumerate()
            {
                ui.label(label);
                ui.add_enabled(
                    self.recording.is_none(),
                    egui::TextEdit::singleline(value).desired_width(ui.available_width()),
                );
                ui.horizontal(|ui| {
                    if ui
                        .button(if self.recording == Some(index) {
                            "Отменить запись"
                        } else {
                            "Записать нажатием"
                        })
                        .clicked()
                    {
                        self.recording = if self.recording == Some(index) {
                            None
                        } else {
                            Some(index)
                        };
                        ui.memory_mut(|m| {
                            if let Some(id) = m.focused() {
                                m.surrender_focus(id);
                            }
                        });
                    }
                    if ui.button("Очистить").clicked() {
                        value.clear();
                        self.recording = None;
                    }
                });
                ui.add_space(8.);
            }
            if self.recording.is_some() {
                ui.label(RichText::new("Нажмите сочетание. Esc — отмена. Глобальные клавиши панели временно отключены.").color(theme::ACCENT));
            }
            ui.label(RichText::new("Например: Ctrl+Alt+F11. Пустое поле отключает сочетание. Если оно занято другой программой, здесь появится сообщение после применения.").small().color(theme::MUTED));
        });
    }
    fn capture_hotkey(&mut self, ctx: &egui::Context) {
        let Some(index) = self.recording else {
            return;
        };
        if !ctx.input(|i| i.focused) {
            self.recording = None;
            return;
        }
        let events = ctx.input_mut(|i| {
            let events = i.events.clone();
            i.events
                .retain(|e| !matches!(e, egui::Event::Key { .. } | egui::Event::Text(_)));
            events
        });
        for event in events {
            if let egui::Event::Key {
                key,
                physical_key,
                pressed: true,
                repeat: false,
                modifiers,
                ..
            } = event
            {
                if key == egui::Key::Escape {
                    self.recording = None;
                    break;
                }
                #[cfg(windows)]
                let win = {
                    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
                        GetKeyState, VK_LWIN, VK_RWIN,
                    };
                    unsafe { GetKeyState(VK_LWIN as i32) < 0 || GetKeyState(VK_RWIN as i32) < 0 }
                };
                #[cfg(not(windows))]
                let win = modifiers.mac_cmd || modifiers.command && !modifiers.ctrl;
                if let Some(text) = recorded_hotkey(physical_key.unwrap_or(key), modifiers, win) {
                    let value = match index {
                        0 => &mut self.draft.hotkeys.visibility,
                        1 => &mut self.draft.hotkeys.lock,
                        _ => &mut self.draft.hotkeys.settings,
                    };
                    *value = text;
                    self.recording = None;
                    self.status = Some("Сочетание записано в черновик".into());
                    break;
                } else {
                    self.status =
                        Some("Эта клавиша не поддерживается. Выберите другое сочетание.".into());
                }
            }
        }
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
        theme::card(
            ui,
            concat!("MH Sidebar ", env!("CARGO_PKG_VERSION")),
            |ui| {
                ui.label("Мониторинг системы для второго экрана.");
                ui.label(RichText::new("Rust · egui · Windows").color(theme::ACCENT));
                ui.label("Оформление и часть системных модулей — MH Monitoring, midnightheartsgames. Шрифт Cuprum.");
                ui.label("Температура и мощность CPU — через драйвер PawnIO (ставится отдельно, нужны права администратора); без него остальное работает без администратора. NVIDIA: NVML; другие GPU: доступные показатели WDDM. Данные не отправляются в сеть.");
                ui.label(
                    RichText::new(mh_sidebar::config::default_path().display().to_string())
                        .small()
                        .color(theme::MUTED),
                );
            },
        );
    }
}

fn recorded_hotkey(key: egui::Key, modifiers: egui::Modifiers, win: bool) -> Option<String> {
    let mut parts = Vec::new();
    if modifiers.ctrl {
        parts.push("Ctrl");
    }
    if modifiers.alt {
        parts.push("Alt");
    }
    if modifiers.shift {
        parts.push("Shift");
    }
    if win {
        parts.push("Super");
    }
    parts.push(match key {
        egui::Key::OpenBracket => "BracketLeft",
        egui::Key::CloseBracket => "BracketRight",
        egui::Key::Equals => "Equal",
        egui::Key::Backtick => "Backquote",
        _ => key.name(),
    });
    let text = parts.join("+");
    crate::controls::parse_hotkey(&text).map(|_| text)
}

#[cfg(test)]
mod usability_tests {
    use super::*;
    #[test]
    fn capture_and_escape_only_change_the_draft() {
        let settings = Settings::default();
        let mut window = SettingsWindow::new(&settings, false);
        let ctx = egui::Context::default();
        let send = |window: &mut SettingsWindow, key| {
            let input = egui::RawInput {
                focused: true,
                events: vec![egui::Event::Key {
                    key,
                    physical_key: Some(key),
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers {
                        ctrl: true,
                        alt: true,
                        ..Default::default()
                    },
                }],
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| window.capture_hotkey(ui.ctx()));
            output.textures_delta.clear();
        };
        window.recording = Some(0);
        send(&mut window, egui::Key::F8);
        assert_eq!(window.draft.hotkeys.visibility, "Ctrl+Alt+F8");
        assert_eq!(settings.hotkeys.visibility, "Ctrl+Alt+F11");
        assert!(window.recording.is_none());
        window.recording = Some(0);
        send(&mut window, egui::Key::Escape);
        assert_eq!(window.draft.hotkeys.visibility, "Ctrl+Alt+F8");
        assert!(window.recording.is_none());
    }
    #[test]
    fn recorded_keys_are_valid_global_hotkeys() {
        let modifiers = egui::Modifiers {
            ctrl: true,
            alt: true,
            ..Default::default()
        };
        assert_eq!(
            recorded_hotkey(egui::Key::F11, modifiers, false).as_deref(),
            Some("Ctrl+Alt+F11")
        );
        for key in [
            egui::Key::A,
            egui::Key::Num0,
            egui::Key::ArrowUp,
            egui::Key::OpenBracket,
            egui::Key::Equals,
        ] {
            assert!(recorded_hotkey(key, modifiers, false).is_some(), "{key:?}");
        }
        assert!(recorded_hotkey(egui::Key::Copy, modifiers, false).is_none());
        assert_eq!(
            recorded_hotkey(egui::Key::F8, egui::Modifiers::default(), true).as_deref(),
            Some("Super+F8")
        );
    }
}
