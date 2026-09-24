use eframe::egui;
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::HotKey};
use mh_sidebar::config::Hotkeys as HotkeySettings;
use std::{
    process::Command as ProcessCommand,
    sync::mpsc::{self, Receiver, Sender},
};
use tray_icon::{
    Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    ToggleVisible,
    ToggleLock,
    OpenSettings,
    RestartSensors,
    Exit,
}

pub struct Controls {
    pub commands: Receiver<Command>,
    sender: Sender<Command>,
    context: egui::Context,
    tray: Option<TrayIcon>,
    visibility_item: MenuItem,
    lock_item: MenuItem,
    settings_item: MenuItem,
    restart_item: MenuItem,
    exit_item: MenuItem,
    manager: Option<GlobalHotKeyManager>,
    bindings: Vec<(HotKey, Command)>,
    pub hotkey_errors: Vec<String>,
}

impl Controls {
    pub fn install(context: &egui::Context, settings: &HotkeySettings) -> Self {
        #[cfg(target_os = "linux")]
        let gtk_ready = gtk::init().is_ok();
        let (sender, commands) = mpsc::channel();
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
        #[cfg(target_os = "linux")]
        let tray = gtk_ready
            .then(|| {
                TrayIconBuilder::new()
                    .with_menu(Box::new(menu))
                    .with_menu_on_left_click(false)
                    .with_tooltip("MH Sidebar")
                    .with_icon(tray_icon())
                    .build()
                    .ok()
            })
            .flatten();
        #[cfg(target_os = "macos")]
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_tooltip("MH Sidebar")
            .with_icon(tray_icon())
            .build()
            .ok();
        let manager = GlobalHotKeyManager::new().ok();
        let mut controls = Self {
            commands,
            sender,
            context: context.clone(),
            tray,
            visibility_item,
            lock_item,
            settings_item,
            restart_item,
            exit_item,
            manager,
            bindings: Vec::new(),
            hotkey_errors: Vec::new(),
        };
        controls.update_hotkeys(settings);
        controls
    }

    pub fn sync_menu(&self, visible: bool, locked: bool) {
        self.visibility_item.set_text(if visible {
            "Скрыть панель"
        } else {
            "Показать панель"
        });
        self.lock_item.set_text(if locked {
            "Разблокировать"
        } else {
            "Заблокировать"
        });
    }

    pub fn set_tooltip(&self, text: &str) {
        if let Some(tray) = &self.tray {
            let _ = tray.set_tooltip(Some(text));
        }
    }

    pub fn notify(&self, title: &str, message: &str) {
        #[cfg(target_os = "linux")]
        let _ = ProcessCommand::new("notify-send")
            .arg(title)
            .arg(message)
            .spawn();
        #[cfg(target_os = "macos")]
        {
            let quote = |value: &str| value.replace('\\', "\\\\").replace('"', "\\\"");
            let script = format!(
                "display notification \"{}\" with title \"{}\"",
                quote(message),
                quote(title)
            );
            let _ = ProcessCommand::new("osascript")
                .arg("-e")
                .arg(script)
                .spawn();
        }
    }

    pub fn update_hotkeys(&mut self, settings: &HotkeySettings) {
        if let Some(manager) = &self.manager {
            for (hotkey, _) in self.bindings.drain(..) {
                let _ = manager.unregister(hotkey);
            }
        }
        let mut errors = Vec::new();
        if let Some(manager) = &self.manager {
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
                        Ok(()) => self.bindings.push((hotkey, command)),
                        Err(_) => errors.push(format!("хоткей {text} занят другой программой")),
                    },
                    None => errors.push(format!("хоткей «{text}» не разобран")),
                }
            }
        } else {
            errors.push("глобальные хоткеи недоступны".into());
        }
        if self.tray.is_none() {
            errors.push("значок в трее не создан".into())
        }
        self.hotkey_errors = errors;
    }

    pub fn suspend_hotkeys(&mut self) -> bool {
        if let Some(manager) = &self.manager {
            for (hotkey, _) in self.bindings.drain(..) {
                let _ = manager.unregister(hotkey);
            }
        }
        true
    }

    pub fn poll_errors(&mut self) -> bool {
        #[cfg(target_os = "linux")]
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            let command = if event.id == self.visibility_item.id().clone() {
                Some(Command::ToggleVisible)
            } else if event.id == self.lock_item.id().clone() {
                Some(Command::ToggleLock)
            } else if event.id == self.settings_item.id().clone() {
                Some(Command::OpenSettings)
            } else if event.id == self.restart_item.id().clone() {
                Some(Command::RestartSensors)
            } else if event.id == self.exit_item.id().clone() {
                Some(Command::Exit)
            } else {
                None
            };
            if let Some(command) = command {
                self.send(command)
            }
        }
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                self.send(Command::ToggleVisible);
            }
        }
        while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if event.state == HotKeyState::Pressed
                && let Some((_, command)) = self
                    .bindings
                    .iter()
                    .find(|(hotkey, _)| hotkey.id() == event.id)
            {
                self.send(*command);
            }
        }
        false
    }

    fn send(&self, command: Command) {
        let _ = self.sender.send(command);
        self.context.request_repaint_of(egui::ViewportId::ROOT);
    }
}

pub fn parse_hotkey(text: &str) -> Option<HotKey> {
    text.parse().ok()
}

fn tray_icon() -> Icon {
    const SIZE: u32 = 32;
    let mut rgba = vec![0u8; (SIZE * SIZE * 4) as usize];
    for (left, top) in [(4, 18), (13, 8), (22, 13)] {
        for y in top..28 {
            for x in left..left + 6 {
                let offset = ((y * SIZE + x) * 4) as usize;
                rgba[offset..offset + 4].copy_from_slice(&[0x3F, 0xD0, 0xD8, 0xFF]);
            }
        }
    }
    Icon::from_rgba(rgba, SIZE, SIZE).expect("valid tray icon")
}
