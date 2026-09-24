use crate::{
    analytics_ui::{AnalyticsView, AnalyticsWindow},
    controls::{Command, Controls},
    settings_ui::{DriverAction, SettingsWindow},
    sidebar::{self, Histories},
    theme,
};
use eframe::egui::{self, ViewportCommand, ViewportId};
use mh_sidebar::{
    alerts::AlertEngine,
    collection::Collector,
    config::{self, Settings},
    model::Snapshot,
    platform::{self, DockWindow, Monitor},
};
#[cfg(windows)]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    path::PathBuf,
    time::{Duration, Instant, SystemTime},
};

pub fn run() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    let instance = if args.iter().any(|s| s == "--wait-instance") {
        platform::SingleInstance::acquire_waiting(Duration::from_secs(10))
    } else {
        platform::SingleInstance::acquire()
    };
    let Some(_instance) = instance else {
        return Ok(());
    };
    let path = args
        .iter()
        .position(|s| s == "--config")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
        .unwrap_or_else(config::default_path);
    let first = !path.exists();
    let loaded = Settings::load(&path);
    let settings = loaded.settings;
    let mut open = first || args.iter().any(|s| s == "--settings");
    let startup_notice =
        if cfg!(windows) && settings.autostart && platform::legacy_autostart_exists() {
            match platform::set_autostart(true) {
                Ok(()) => {
                    Some("Автозапуск перенесён в Планировщик задач с правами администратора".into())
                }
                Err(e) => {
                    open = true;
                    Some(format!("Автозапуск пока без прав администратора: {e}"))
                }
            }
        } else {
            None
        };
    let smoke = args
        .iter()
        .position(|s| s == "--smoke-test")
        .and_then(|i| args.get(i + 1))
        .and_then(|s| s.parse::<u64>().ok());
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("MH Sidebar")
            .with_decorations(false)
            .with_transparent(true)
            .with_taskbar(false)
            .with_resizable(false)
            .with_active(false)
            .with_inner_size([settings.width, 800.])
            .with_visible(settings.visible || open)
            .with_icon(icon()),
        renderer: eframe::Renderer::Glow,
        persist_window: false,
        ..Default::default()
    };
    eframe::run_native(
        "MH Sidebar",
        options,
        Box::new(move |cc| {
            theme::setup(&cc.egui_ctx);
            #[cfg(windows)]
            let window = cc.window_handle().ok().and_then(|h| match h.as_raw() {
                RawWindowHandle::Win32(h) => Some(unsafe { DockWindow::new(h.hwnd.get() as _) }),
                _ => None,
            });
            #[cfg(not(windows))]
            let window = Some(DockWindow::new(cc.egui_ctx.clone()));
            let mut settings_window = SettingsWindow::new(&settings, first);
            settings_window.open = open;
            settings_window.status = startup_notice.or(loaded.notice);
            let controls = Controls::install(&cc.egui_ctx, &settings.hotkeys);
            let wake_context = cc.egui_ctx.clone();
            let worker = Collector::start(settings.interval_ms, move || {
                wake_context.request_repaint_of(ViewportId::ROOT)
            })
            .map_err(std::io::Error::other)?;
            let mut app = App {
                settings,
                settings_window,
                path,
                writable: loaded.writable,
                controls: Some(controls),
                worker: Some(worker),
                window,
                monitors: current_monitors(&cc.egui_ctx),
                next_monitors: Instant::now(),
                snapshot: Snapshot::default(),
                histories: Histories::default(),
                start: Instant::now(),
                start_wall: SystemTime::now(),
                analytics_window: AnalyticsWindow::default(),
                alert_engine: AlertEngine::default(),
                #[cfg(debug_assertions)]
                analytics_qa_opened: false,
                exiting: false,
                smoke,
                last_viewport: None,
                hotkeys_suspended: false,
            };
            app.control_errors();
            app.sync(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
}

#[cfg(windows)]
fn current_monitors(_: &egui::Context) -> Vec<Monitor> {
    platform::monitors()
}

#[cfg(not(windows))]
fn current_monitors(context: &egui::Context) -> Vec<Monitor> {
    let found = platform::monitors();
    if !found.is_empty() {
        return found;
    }
    context
        .input(|input| input.viewport().monitor_size)
        .map_or_else(Vec::new, |size| {
            vec![Monitor {
                id: "current".into(),
                name: "Текущий экран".into(),
                rect: platform::ScreenRect {
                    left: 0,
                    top: 0,
                    right: size.x as i32,
                    bottom: size.y as i32,
                },
                work: platform::ScreenRect {
                    left: 0,
                    top: 0,
                    right: size.x as i32,
                    bottom: size.y as i32,
                },
                scale: 1.0,
                primary: true,
            }]
        })
}

fn autostart_needs_update(previous: bool, next: bool, first_run: bool, legacy: bool) -> bool {
    if first_run {
        next || legacy
    } else {
        previous != next || (next && legacy)
    }
}

struct App {
    settings: Settings,
    settings_window: SettingsWindow,
    path: PathBuf,
    writable: bool,
    controls: Option<Controls>,
    worker: Option<Collector>,
    window: Option<DockWindow>,
    monitors: Vec<Monitor>,
    next_monitors: Instant,
    snapshot: Snapshot,
    histories: Histories,
    start: Instant,
    start_wall: SystemTime,
    analytics_window: AnalyticsWindow,
    alert_engine: AlertEngine,
    #[cfg(debug_assertions)]
    analytics_qa_opened: bool,
    exiting: bool,
    smoke: Option<u64>,
    last_viewport: Option<(bool, bool)>,
    hotkeys_suspended: bool,
}
impl App {
    fn control_errors(&mut self) {
        if let Some(c) = &self.controls {
            if !c.hotkey_errors.is_empty() {
                self.settings_window.status = Some(c.hotkey_errors.join("; "));
            }
            if c.hotkey_errors.iter().any(|e| e.contains("тре")) {
                self.settings.visible = true;
                self.settings.locked = false;
                self.settings.show_header = true;
                self.settings_window.open = true;
            }
        }
    }
    fn command(&mut self, command: Command) {
        match command {
            Command::ToggleVisible => self.settings.visible = !self.settings.visible,
            Command::ToggleLock => self.settings.locked = !self.settings.locked,
            Command::OpenSettings => self.settings_window.open(&self.settings),
            Command::RestartSensors => {
                if let Some(w) = &mut self.worker {
                    w.restart();
                }
                self.histories.clear();
                self.alert_engine.clear();
            }
            Command::Exit => self.exiting = true,
        }
        if matches!(command, Command::ToggleVisible | Command::ToggleLock) {
            if self.settings_window.open {
                self.settings_window.draft.visible = self.settings.visible;
                self.settings_window.draft.locked = self.settings.locked;
            }
            self.persist();
        }
    }
    fn driver_action(&mut self, action: DriverAction) {
        let result = match action {
            DriverAction::Install => platform::install_pawnio().map(|_| {
                "Установка PawnIO запущена. Когда она завершится, перезапустите MH Sidebar от имени администратора."
            }),
            DriverAction::OpenSite => platform::open_url("https://pawnio.eu/").map(|_| "Открыт сайт PawnIO"),
            DriverAction::RestartElevated => platform::relaunch_elevated().map(|_| {
                self.exiting = true;
                "Перезапуск от имени администратора…"
            }),
        };
        self.settings_window.status = Some(match result {
            Ok(message) => message.into(),
            Err(e) => format!("Не удалось: {e}"),
        });
    }
    fn persist(&mut self) {
        if self.writable
            && let Err(e) = self.settings.save(&self.path)
        {
            self.settings_window.status = Some(format!("Не удалось сохранить настройки: {e}"));
        }
    }
    fn apply(&mut self, mut next: Settings) {
        if !self.writable {
            self.settings_window.status =
                Some("Сохранение недоступно: проверьте исходный файл настроек.".into());
            return;
        }
        next.normalize();
        let mut keys = Vec::new();
        for key in [
            &next.hotkeys.visibility,
            &next.hotkeys.lock,
            &next.hotkeys.settings,
        ] {
            if key.trim().is_empty() {
                continue;
            }
            let Some(parsed) = crate::controls::parse_hotkey(key) else {
                self.settings_window.status = Some(format!("Неизвестное сочетание: {key}"));
                return;
            };
            if keys.contains(&parsed.id()) {
                self.settings_window.status =
                    Some("Для разных действий нужны разные сочетания клавиш.".into());
                return;
            }
            keys.push(parsed.id());
        }
        let startup_changed = next.autostart != self.settings.autostart;
        let startup_needs_update = autostart_needs_update(
            self.settings.autostart,
            next.autostart,
            self.settings_window.first_run,
            platform::legacy_autostart_exists(),
        );
        if startup_needs_update && let Err(e) = platform::set_autostart(next.autostart) {
            self.settings_window.status = Some(format!("Автозапуск: {e}"));
            return;
        }
        if let Err(e) = next.save(&self.path) {
            if startup_needs_update && (startup_changed || self.settings_window.first_run) {
                let rollback = self.settings.autostart && !self.settings_window.first_run;
                let _ = platform::set_autostart(rollback);
            }
            self.settings_window.status = Some(format!("Не удалось сохранить настройки: {e}"));
            return;
        }
        let changed_keys = next.hotkeys != self.settings.hotkeys;
        if next.alert_rules != self.settings.alert_rules
            || next.notifications_enabled != self.settings.notifications_enabled
        {
            self.alert_engine.clear();
        }
        self.settings = next;
        if let Some(w) = &mut self.worker {
            w.set_interval(self.settings.interval_ms);
        }
        self.settings_window.status = Some("Настройки сохранены".into());
        if changed_keys
            && !self.hotkeys_suspended
            && let Some(controls) = &mut self.controls
        {
            controls.update_hotkeys(&self.settings.hotkeys);
        }
        self.control_errors();
        if self.settings_window.first_run {
            self.settings_window.first_run = false;
            self.settings_window.open = false;
        }
    }
    fn sync(&mut self, ctx: &egui::Context) {
        let mut effective = self.settings.clone();
        effective.visible = self.settings.visible || self.settings_window.open;
        effective.reserve_space = self.settings.reserve_space && self.settings.visible;
        effective.locked = self.settings.locked || !self.settings.visible;
        let state = (effective.visible, effective.locked);
        if self.last_viewport != Some(state) {
            ctx.send_viewport_cmd(ViewportCommand::Visible(effective.visible));
            ctx.send_viewport_cmd(ViewportCommand::MousePassthrough(effective.locked));
            self.last_viewport = Some(state);
        }
        if let Some(window) = &mut self.window
            && let Err(e) = window.tick(&effective, &self.monitors)
        {
            self.settings_window.status = Some(e);
        }
        if let Some(c) = &self.controls {
            c.sync_menu(self.settings.visible, self.settings.locked);
        }
    }
    fn sample(&mut self) {
        let now = Instant::now();
        let Some(worker) = &mut self.worker else {
            return;
        };
        self.snapshot = worker.snapshot(now);
        if self.settings.notifications_enabled {
            for alert in self
                .alert_engine
                .observe(&self.snapshot, &self.settings.alert_rules, now)
            {
                if let Some(c) = &self.controls {
                    c.notify(
                        "MH Sidebar — предупреждение",
                        &format!(
                            "{} · {} · {}: {:.1} {} (порог {:.1})",
                            alert.block.title(),
                            alert.device,
                            alert.label,
                            alert.value,
                            alert.unit,
                            alert.threshold
                        ),
                    );
                }
            }
        } else {
            self.alert_engine.clear();
        }
        self.histories.observe(
            &self.snapshot,
            now,
            self.start,
            Duration::from_secs(self.settings.graph_seconds),
        );
        #[cfg(debug_assertions)]
        if !self.analytics_qa_opened
            && std::env::var_os("MH_SIDEBAR_ANALYTICS_QA").is_some()
            && let Some(section) = self
                .snapshot
                .sections
                .iter()
                .find(|s| s.rows.iter().any(|r| r.key == "load"))
            && let Some(row) = section.rows.iter().find(|r| r.key == "load")
        {
            self.analytics_window.open(crate::sidebar::MetricTarget {
                block: section.id,
                device_id: section.device_id.clone(),
                metric: row.key.clone(),
                device: section.device.clone(),
                label: row.label.clone(),
                unit: row.unit.clone(),
            });
            self.analytics_qa_opened = true;
        }
        let migrated = self.settings.resolve_devices(&self.snapshot);
        if self.settings_window.open {
            self.settings_window.draft.resolve_devices(&self.snapshot);
        }
        if migrated {
            self.persist();
        }
        let retained = self.histories.missing_sections(&self.snapshot);
        self.snapshot.merge(Snapshot {
            sections: retained,
            ..Default::default()
        });
        if let Some(c) = &self.controls {
            let loads = self
                .snapshot
                .sections
                .iter()
                .filter_map(|s| {
                    s.rows
                        .iter()
                        .find(|r| r.key == "load")
                        .map(|r| format!("{} {}", s.id.short(), r.text))
                })
                .take(3)
                .collect::<Vec<_>>()
                .join(" · ");
            c.set_tooltip(&format!("MH Sidebar\n{loads}"));
        }
    }
}
impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.controls.as_mut().is_some_and(Controls::poll_errors) {
            self.control_errors();
        }
        loop {
            let command = self
                .controls
                .as_ref()
                .and_then(|c| c.commands.try_recv().ok());
            if let Some(c) = command {
                self.command(c);
            } else {
                break;
            }
        }
        if self
            .smoke
            .is_some_and(|seconds| self.start.elapsed() >= Duration::from_secs(seconds))
        {
            self.exiting = true;
        }
        if ctx.input(|i| i.viewport().close_requested()) && !self.exiting {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.settings.visible = false;
        }
        if Instant::now() >= self.next_monitors {
            self.monitors = current_monitors(ctx);
            self.next_monitors = Instant::now() + Duration::from_secs(3);
        }
        self.sample();
        self.sync(ctx);
        ctx.request_repaint_after(Duration::from_secs(1));
        if self.exiting {
            self.window.take();
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = self.start.elapsed().as_secs_f64();
        if self.settings.visible {
            let [r, g, b] = self.settings.background;
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::new()
                        .fill(egui::Color32::from_rgba_unmultiplied(
                            r,
                            g,
                            b,
                            (self.settings.opacity * 255.) as u8,
                        ))
                        .inner_margin(12),
                )
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let actions = sidebar::show(
                                ui,
                                &self.snapshot,
                                &self.settings,
                                &self.histories,
                                now,
                                true,
                            );
                            if actions.settings {
                                self.command(Command::OpenSettings);
                            }
                            if actions.hide {
                                self.command(Command::ToggleVisible);
                            }
                            if actions.lock {
                                self.command(Command::ToggleLock);
                            }
                            if let Some(target) = actions.detail {
                                self.analytics_window.open(target);
                            }
                        });
                });
        }
        if let Some(next) = self.settings_window.show(
            &ctx,
            &self.snapshot,
            &self.histories,
            now,
            &self.monitors,
            &self.path,
        ) {
            self.apply(next);
        }
        self.analytics_window.show(
            &ctx,
            AnalyticsView {
                snapshot: &self.snapshot,
                histories: &self.histories,
                now,
                seconds: self.settings.graph_seconds,
                origin: self.start_wall,
                active_path: &self.path,
            },
        );
        let recording = self.settings_window.open && self.settings_window.recording.is_some();
        if recording != self.hotkeys_suspended {
            if let Some(controls) = &mut self.controls {
                if recording {
                    if !controls.suspend_hotkeys() {
                        self.settings_window.recording = None;
                        self.settings_window.status =
                            Some("Не удалось начать запись клавиш. Попробуйте ещё раз.".into());
                        controls.update_hotkeys(&self.settings.hotkeys);
                    } else {
                        self.hotkeys_suspended = true;
                    }
                } else {
                    controls.update_hotkeys(&self.settings.hotkeys);
                    self.hotkeys_suspended = false;
                }
            } else {
                self.settings_window.recording = None;
            }
        }
        if let Some(action) = self.settings_window.action.take() {
            self.driver_action(action);
        }
        #[cfg(debug_assertions)]
        if let Some(gl) = _frame.gl() {
            if self.analytics_window.is_open() {
                crate::capture::immediate(&ctx, gl, now, "analytics");
            } else if self.settings_window.open {
                crate::capture::immediate(&ctx, gl, now, "settings");
            }
        }
        self.sync(&ctx);
        #[cfg(debug_assertions)]
        crate::capture::frame(ui, "sidebar", now);
    }
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.; 4]
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.window.take();
        self.worker.take();
        self.controls.take();
    }
}

fn icon() -> egui::IconData {
    let mut rgba = vec![0; 32 * 32 * 4];
    for (left, top) in [(4, 18), (13, 7), (22, 12)] {
        for y in top..28 {
            for x in left..left + 6 {
                let offset = (y * 32 + x) * 4;
                rgba[offset..offset + 4].copy_from_slice(&[63, 208, 216, 255]);
            }
        }
    }
    egui::IconData {
        rgba,
        width: 32,
        height: 32,
    }
}

#[cfg(test)]
mod autostart_tests {
    use super::*;

    #[test]
    fn first_run_opt_out_never_requests_elevation_without_an_old_entry() {
        assert!(!autostart_needs_update(true, false, true, false));
        assert!(autostart_needs_update(true, true, true, false));
        assert!(autostart_needs_update(true, false, true, true));
    }
}
