use crate::{
    controls::{Command, Controls},
    settings_ui::{DriverAction, SettingsWindow},
    sidebar::{self, Histories},
    theme,
};
use eframe::egui::{self, ViewportCommand, ViewportId};
use mh_sidebar::{
    config::{self, Settings},
    model::Snapshot,
    platform::{self, DockWindow, Monitor},
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        mpsc::{self, Sender},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

enum WorkerRequest {
    Interval(u64),
    Restart,
    Stop,
}
struct Worker {
    latest: Arc<Mutex<Option<(Instant, Snapshot)>>>,
    sender: Sender<WorkerRequest>,
    thread: Option<JoinHandle<()>>,
}
impl Worker {
    fn start(ctx: &egui::Context, interval: u64) -> Result<Self, String> {
        let latest = Arc::new(Mutex::new(None));
        let output = latest.clone();
        let ctx = ctx.clone();
        let (sender, rx) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("mh-sensors".into())
            .spawn(move || {
                let mut sampler = mh_sidebar::sensors::Sampler::new();
                let mut interval = interval;
                loop {
                    let snapshot = sampler.sample();
                    if let Ok(mut slot) = output.lock() {
                        *slot = Some((Instant::now(), snapshot));
                    }
                    ctx.request_repaint_of(ViewportId::ROOT);
                    match rx.recv_timeout(Duration::from_millis(interval)) {
                        Ok(WorkerRequest::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                            break;
                        }
                        Ok(WorkerRequest::Interval(ms)) => interval = ms,
                        Ok(WorkerRequest::Restart) => sampler = mh_sidebar::sensors::Sampler::new(),
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            latest,
            sender,
            thread: Some(thread),
        })
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.sender.send(WorkerRequest::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

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
    let open = first || args.iter().any(|s| s == "--settings");
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
            let window = cc.window_handle().ok().and_then(|h| match h.as_raw() {
                // The eframe root window is live and belongs to this GUI thread.
                RawWindowHandle::Win32(h) => Some(unsafe { DockWindow::new(h.hwnd.get() as _) }),
                _ => None,
            });
            let mut settings_window = SettingsWindow::new(&settings, first);
            settings_window.open = open;
            settings_window.status = loaded.notice;
            let controls = Controls::install(&cc.egui_ctx, &settings.hotkeys);
            let worker =
                Worker::start(&cc.egui_ctx, settings.interval_ms).map_err(std::io::Error::other)?;
            let mut app = App {
                settings,
                settings_window,
                path,
                writable: loaded.writable,
                controls: Some(controls),
                worker: Some(worker),
                window,
                monitors: platform::monitors(),
                next_monitors: Instant::now(),
                snapshot: Snapshot::default(),
                histories: Histories::new(),
                start: Instant::now(),
                last_sample: None,
                stale: false,
                exiting: false,
                smoke,
                last_viewport: None,
            };
            app.control_errors();
            app.sync(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
}

struct App {
    settings: Settings,
    settings_window: SettingsWindow,
    path: PathBuf,
    writable: bool,
    controls: Option<Controls>,
    worker: Option<Worker>,
    window: Option<DockWindow>,
    monitors: Vec<Monitor>,
    next_monitors: Instant,
    snapshot: Snapshot,
    histories: Histories,
    start: Instant,
    last_sample: Option<Instant>,
    stale: bool,
    exiting: bool,
    smoke: Option<u64>,
    last_viewport: Option<(bool, bool)>,
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
                if let Some(w) = &self.worker {
                    let _ = w.sender.send(WorkerRequest::Restart);
                }
                self.histories.clear();
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
        if startup_changed && let Err(e) = platform::set_autostart(next.autostart) {
            self.settings_window.status = Some(format!("Автозапуск: {e}"));
            return;
        }
        if let Err(e) = next.save(&self.path) {
            if startup_changed {
                let _ = platform::set_autostart(self.settings.autostart);
            }
            self.settings_window.status = Some(format!("Не удалось сохранить настройки: {e}"));
            return;
        }
        let changed_keys = next.hotkeys != self.settings.hotkeys;
        self.settings = next;
        if let Some(w) = &self.worker {
            let _ = w
                .sender
                .send(WorkerRequest::Interval(self.settings.interval_ms));
        }
        self.settings_window.status = Some("Настройки сохранены".into());
        if changed_keys && let Some(controls) = &self.controls {
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
        // Keep the invisible root alive while its independent settings viewport is open.
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
        let latest = self
            .worker
            .as_ref()
            .and_then(|w| w.latest.lock().ok()?.take());
        if let Some((at, snapshot)) = latest {
            let now = at.duration_since(self.start).as_secs_f64();
            let mut active = std::collections::HashSet::new();
            for section in &snapshot.sections {
                for row in &section.rows {
                    let key = sidebar::key(section.id, &section.device, &row.key);
                    active.insert(key.clone());
                    self.histories.entry(key).or_default().push(now, row.value);
                }
            }
            self.histories.retain(|k, _| active.contains(k));
            self.snapshot = snapshot;
            self.last_sample = Some(at);
            self.stale = false;
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
        if !self.stale
            && self.last_sample.is_some_and(|at| {
                at.elapsed() > Duration::from_millis((self.settings.interval_ms * 3).max(5000))
            })
        {
            for section in &mut self.snapshot.sections {
                for row in &mut section.rows {
                    row.value = None;
                    row.text = "—".into();
                    row.reason = Some("Данные устарели: ожидание датчика".into());
                }
            }
            let now = self.start.elapsed().as_secs_f64();
            for h in self.histories.values_mut() {
                h.push(now, None);
            }
            self.stale = true;
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
            self.monitors = platform::monitors();
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
                        });
                });
        }
        if let Some(next) =
            self.settings_window
                .show(&ctx, &self.snapshot, &self.histories, now, &self.monitors)
        {
            self.apply(next);
        }
        if let Some(action) = self.settings_window.action.take() {
            self.driver_action(action);
        }
        #[cfg(debug_assertions)]
        if self.settings_window.open
            && let Some(gl) = _frame.gl()
        {
            crate::capture::immediate(&ctx, gl, now);
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
