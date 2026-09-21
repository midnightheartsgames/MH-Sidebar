//! Трей и глобальные хоткеи. Оба присылают команды в один канал.
//!
//! **Живут в своём потоке** со своим циклом сообщений. В потоке winit модальное меню трея
//! (`TrackPopupMenu`) подвешивало цикл событий: HUD замирал, а «Настройки» и «Выход» ждали
//! первого движения мыши над оверлеем (PLAN.md §2.16).
//!
//! Обработчики событий будят UI: пока панель скрыт, eframe не рисует кадры, и без пробуждения
//! команду «показать» было бы некому выполнить.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;

use eframe::egui;
use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};

use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

use mh_sidebar::config::Hotkeys as HotkeySettings;

/// Как часто поток трея проверяет просьбы UI, если сообщений нет.
const TRAY_TICK_MS: u32 = 50;

fn pump_messages(wait_ms: u32) {
    use windows_sys::Win32::UI::WindowsAndMessaging::*;
    unsafe {
        MsgWaitForMultipleObjectsEx(
            0,
            std::ptr::null(),
            wait_ms,
            QS_ALLINPUT,
            MWMO_INPUTAVAILABLE,
        );
        let mut msg: MSG = std::mem::zeroed();
        while PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    ToggleVisible,
    ToggleLock,
    OpenSettings,
    RestartSensors,
    Exit,
}

/// Просьбы UI к потоку трея.
enum Request {
    Menu { visible: bool, locked: bool },
    Tooltip(String),
    Hotkeys(HotkeySettings),
    Quit,
}

pub struct Controls {
    pub commands: Receiver<Command>,
    requests: Sender<Request>,
    thread: Option<JoinHandle<()>>,
    /// Хоткеи, которые не удалось зарегистрировать, — для подсказки пользователю.
    pub hotkey_errors: Vec<String>,
    errors: Receiver<Vec<String>>,
}

impl Controls {
    pub fn install(ctx: &egui::Context, hotkeys: &HotkeySettings) -> Controls {
        let (sender, commands) = channel();
        let (requests, inbox) = channel::<Request>();
        let (ready, started) = channel::<Vec<String>>();
        let ctx = ctx.clone();
        let hotkeys = hotkeys.clone();

        let thread = std::thread::Builder::new()
            .name("mh-tray".into())
            .spawn(move || tray_thread(&ctx, &hotkeys, &sender, &inbox, &ready))
            .ok();
        // Ошибки хоткеев нужны HUD сразу. Поток отвечает, как только всё создано.
        let hotkey_errors = if thread.is_some() {
            started
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap_or_else(|_| vec!["трей не запустился".to_string()])
        } else {
            vec!["трей не запустился".to_string()]
        };
        Controls {
            commands,
            requests,
            thread,
            hotkey_errors,
            errors: started,
        }
    }

    /// Подписи пунктов меню следуют состоянию панели.
    pub fn sync_menu(&self, visible: bool, locked: bool) {
        let _ = self.requests.send(Request::Menu { visible, locked });
    }

    /// Подсказка значка в трее — единственное, что видно поверх эксклюзивного полноэкранного
    /// режима.
    pub fn set_tooltip(&self, text: &str) {
        let _ = self.requests.send(Request::Tooltip(text.to_string()));
    }
    pub fn update_hotkeys(&self, settings: &HotkeySettings) {
        let _ = self.requests.send(Request::Hotkeys(settings.clone()));
    }
    pub fn poll_errors(&mut self) -> bool {
        let mut changed = false;
        while let Ok(errors) = self.errors.try_recv() {
            self.hotkey_errors = errors;
            changed = true;
        }
        changed
    }
}

impl Drop for Controls {
    fn drop(&mut self) {
        let _ = self.requests.send(Request::Quit);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Всё, что создаёт скрытые окна, — здесь: окна принадлежат потоку, в котором созданы, и
/// уничтожаться должны в нём же.
fn tray_thread(
    ctx: &egui::Context,
    hotkeys: &HotkeySettings,
    sender: &Sender<Command>,
    inbox: &Receiver<Request>,
    ready: &Sender<Vec<String>>,
) {
    let visibility_item = MenuItem::new("Скрыть панель", true, None);
    let lock_item = MenuItem::new("Заблокировать", true, None);
    let settings_item = MenuItem::new("Настройки…", true, None);
    let restart_item = MenuItem::new("Перезапустить датчики", true, None);
    let exit_item = MenuItem::new("Выход", true, None);
    let menu = Menu::new();
    let _ = menu.append_items(&[
        &visibility_item,
        &lock_item,
        &PredefinedMenuItem::separator(),
        &settings_item,
        &restart_item,
        &PredefinedMenuItem::separator(),
        &exit_item,
    ]);

    let ids = (
        visibility_item.id().clone(),
        lock_item.id().clone(),
        settings_item.id().clone(),
        exit_item.id().clone(),
        restart_item.id().clone(),
    );
    let menu_sender = sender.clone();
    let menu_ctx = ctx.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let command = if event.id == ids.0 {
            Command::ToggleVisible
        } else if event.id == ids.1 {
            Command::ToggleLock
        } else if event.id == ids.2 {
            Command::OpenSettings
        } else if event.id == ids.3 {
            Command::Exit
        } else if event.id == ids.4 {
            Command::RestartSensors
        } else {
            return;
        };
        send(&menu_sender, &menu_ctx, command);
    }));

    // Левый клик по значку — показать или скрыть; меню — по правому.
    let tray_sender = sender.clone();
    let tray_ctx = ctx.clone();
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = event
        {
            send(&tray_sender, &tray_ctx, Command::ToggleVisible);
        }
    }));

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip("MH Sidebar")
        .with_icon(tray_icon())
        .build()
        .ok();

    let manager = GlobalHotKeyManager::new().ok();
    let mut bindings = Vec::new();
    let mut errors = register_hotkeys(manager.as_ref(), &mut bindings, hotkeys);
    if tray.is_none() {
        errors.push("значок в трее не создан".to_string());
    }
    let _ = ready.send(errors);

    loop {
        pump_messages(TRAY_TICK_MS);
        while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if event.state == HotKeyState::Pressed
                && let Some((_, command)) = bindings.iter().find(|(key, _)| key.id() == event.id)
            {
                send(sender, ctx, *command);
            }
        }
        let mut quit = false;
        while let Ok(request) = inbox.try_recv() {
            match request {
                Request::Menu { visible, locked } => {
                    visibility_item.set_text(if visible {
                        "Скрыть панель"
                    } else {
                        "Показать панель"
                    });
                    lock_item.set_text(if locked {
                        "Разблокировать"
                    } else {
                        "Заблокировать"
                    });
                }
                Request::Tooltip(text) => {
                    if let Some(tray) = &tray {
                        let _ = tray.set_tooltip(Some(text));
                    }
                }
                Request::Quit => quit = true,
                Request::Hotkeys(settings) => {
                    let mut errors = register_hotkeys(manager.as_ref(), &mut bindings, &settings);
                    if tray.is_none() {
                        errors.push("значок в трее не создан".into());
                    }
                    let _ = ready.send(errors);
                    ctx.request_repaint_of(egui::ViewportId::ROOT);
                }
            }
        }
        if quit {
            break;
        }
    }
    // Значок и хоткеи снимаются здесь, в своём потоке.
    drop(tray);
    drop(manager);
}

fn send(sender: &Sender<Command>, ctx: &egui::Context, command: Command) {
    let _ = sender.send(command);
    // Будим именно корневое окно: команды разбираются в его `logic`.
    ctx.request_repaint_of(egui::ViewportId::ROOT);
}

fn register_hotkeys(
    manager: Option<&GlobalHotKeyManager>,
    bindings: &mut Vec<(HotKey, Command)>,
    settings: &HotkeySettings,
) -> Vec<String> {
    let mut errors = Vec::new();
    let Some(manager) = manager else {
        errors.push("глобальные хоткеи недоступны".to_string());
        return errors;
    };
    for (key, _) in bindings.drain(..) {
        let _ = manager.unregister(key);
    }
    for (text, command) in [
        (&settings.visibility, Command::ToggleVisible),
        (&settings.lock, Command::ToggleLock),
        (&settings.settings, Command::OpenSettings),
    ] {
        if text.trim().is_empty() {
            continue;
        }
        match parse_hotkey(text) {
            Some(hotkey) => match manager.register(hotkey) {
                Ok(()) => bindings.push((hotkey, command)),
                // Обычно — занят другой программой.
                Err(_) => errors.push(format!("хоткей {text} занят другой программой")),
            },
            None => errors.push(format!("хоткей «{text}» не разобран")),
        }
    }
    errors
}

/// «Ctrl+Shift+F11» → [`HotKey`]. Разбор global-hotkey понимает «ctrl», «shift», «alt».
pub fn parse_hotkey(text: &str) -> Option<HotKey> {
    text.parse().ok()
}

/// Значок трея: три столбика акцентного цвета, как у старого `TrayIconPainter`.
fn tray_icon() -> Icon {
    const SIZE: u32 = 32;
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    let bars = [(4, 18), (13, 8), (22, 13)]; // (x, верх столбика)
    for (left, top) in bars {
        for y in top..28 {
            for x in left..left + 6 {
                let offset = ((y * SIZE + x) * 4) as usize;
                rgba[offset..offset + 4].copy_from_slice(&[0x3F, 0xD0, 0xD8, 0xFF]);
            }
        }
    }
    Icon::from_rgba(rgba, SIZE, SIZE).expect("размеры значка верны")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_hotkeys_parse() {
        let defaults = HotkeySettings::default();
        let visibility = parse_hotkey(&defaults.visibility).expect("Ctrl+Shift+F11");
        let lock = parse_hotkey(&defaults.lock).expect("Ctrl+Shift+F10");
        assert_ne!(visibility.id(), lock.id());
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(parse_hotkey("Ctrl+Nonsense").is_none());
        assert!(parse_hotkey("").is_none());
    }

    #[test]
    fn changing_hotkeys_twice_keeps_the_original_command_channel_alive() {
        use std::time::Duration;
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        let mut settings = HotkeySettings {
            visibility: "Ctrl+Alt+Shift+F21".into(),
            lock: String::new(),
            settings: String::new(),
        };
        let controls = Controls::install(&egui::Context::default(), &settings);
        let class: Vec<u16> = "global_hotkey_app".encode_utf16().chain(Some(0)).collect();
        let mut hwnd = std::ptr::null_mut();
        loop {
            hwnd = unsafe {
                FindWindowExW(std::ptr::null_mut(), hwnd, class.as_ptr(), std::ptr::null())
            };
            assert!(!hwnd.is_null(), "our hotkey manager window must exist");
            let mut pid = 0;
            unsafe {
                GetWindowThreadProcessId(hwnd, &mut pid);
            }
            if pid == std::process::id() {
                break;
            }
        }
        let post = |key: &str| {
            let id = parse_hotkey(key).unwrap().id();
            assert_ne!(unsafe { PostMessageW(hwnd, WM_HOTKEY, id as usize, 0) }, 0);
        };
        post(&settings.visibility);
        assert_eq!(
            controls
                .commands
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            Command::ToggleVisible
        );
        for key in ["Ctrl+Alt+Shift+F22", "Ctrl+Alt+Shift+F23"] {
            let old = settings.visibility.clone();
            settings.visibility = key.into();
            controls.update_hotkeys(&settings);
            let errors = controls
                .errors
                .recv_timeout(Duration::from_secs(2))
                .unwrap();
            assert!(!errors.iter().any(|e| e.contains("хоткей")), "{errors:?}");
            post(&old);
            assert!(
                controls
                    .commands
                    .recv_timeout(Duration::from_millis(120))
                    .is_err()
            );
            post(&settings.visibility);
            assert_eq!(
                controls
                    .commands
                    .recv_timeout(Duration::from_secs(2))
                    .unwrap(),
                Command::ToggleVisible
            );
        }
    }
}
