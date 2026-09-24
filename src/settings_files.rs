use mh_sidebar::config::Settings;
use std::{
    io::Read,
    path::{Path, PathBuf},
};

pub fn import() -> Result<Option<Settings>, String> {
    let Some(path) = dialog(false, false)? else {
        return Ok(None);
    };
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(1_048_577)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    Settings::import(&bytes).map(Some)
}

pub fn export(settings: &Settings, active: &Path) -> Result<bool, String> {
    let Some(path) = dialog(true, false)? else {
        return Ok(false);
    };
    if same_destination(&path, active) {
        return Err(
            "Выберите другой файл для экспорта. Текущие настройки сохраняются кнопкой «Применить»."
                .into(),
        );
    }
    settings.save(&path)?;
    Ok(true)
}

pub fn save_csv_path(active: &Path) -> Result<Option<PathBuf>, String> {
    let Some(path) = dialog(true, true)? else {
        return Ok(None);
    };
    if same_destination(&path, active) {
        return Err(
            "Нельзя записать историю поверх действующих настроек. Выберите другой файл.".into(),
        );
    }
    Ok(Some(path))
}

fn same_destination(a: &Path, b: &Path) -> bool {
    let resolve = |p: &Path| {
        p.canonicalize()
            .or_else(|_| -> std::io::Result<PathBuf> {
                let absolute = std::path::absolute(p)?;
                match (absolute.parent(), absolute.file_name()) {
                    (Some(parent), Some(name)) => Ok(parent
                        .canonicalize()
                        .unwrap_or_else(|_| parent.to_owned())
                        .join(name)),
                    _ => Ok(absolute),
                }
            })
            .map(|p| {
                let path = p.to_string_lossy().to_string();
                if cfg!(windows) {
                    path.to_lowercase()
                } else {
                    path
                }
            })
    };
    matches!((resolve(a), resolve(b)), (Ok(a), Ok(b)) if a == b)
}

#[cfg(windows)]
fn dialog(save: bool, csv: bool) -> Result<Option<PathBuf>, String> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::UI::{Controls::Dialogs::*, Input::KeyboardAndMouse::GetActiveWindow};
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let filter = wide(if csv {
        "Данные CSV\0*.csv\0\0"
    } else {
        "Настройки JSON\0*.json\0\0"
    });
    let extension = wide(if csv { "csv" } else { "json" });
    let title = wide(if csv {
        "Экспорт истории"
    } else if save {
        "Экспорт черновика настроек"
    } else {
        "Импорт настроек в черновик"
    });
    let mut file = vec![0u16; 32768];
    if save {
        let name = wide(if csv {
            "MH-Sidebar-history.csv"
        } else {
            "MH-Sidebar-settings.json"
        });
        file[..name.len()].copy_from_slice(&name);
    }
    unsafe {
        let mut options: OPENFILENAMEW = std::mem::zeroed();
        options.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
        options.hwndOwner = GetActiveWindow();
        options.lpstrFilter = filter.as_ptr();
        options.lpstrDefExt = extension.as_ptr();
        options.lpstrTitle = title.as_ptr();
        options.lpstrFile = file.as_mut_ptr();
        options.nMaxFile = file.len() as u32;
        options.Flags = OFN_EXPLORER
            | OFN_NOCHANGEDIR
            | OFN_PATHMUSTEXIST
            | if save {
                OFN_OVERWRITEPROMPT
            } else {
                OFN_FILEMUSTEXIST
            };
        let ok = if save {
            GetSaveFileNameW(&mut options)
        } else {
            GetOpenFileNameW(&mut options)
        };
        if ok == 0 {
            let error = CommDlgExtendedError();
            return if error == 0 {
                Ok(None)
            } else {
                Err(format!("Диалог выбора файла: 0x{error:x}"))
            };
        }
    }
    let len = file.iter().position(|c| *c == 0).unwrap_or(file.len());
    Ok(Some(std::ffi::OsString::from_wide(&file[..len]).into()))
}

#[cfg(not(windows))]
fn dialog(save: bool, csv: bool) -> Result<Option<PathBuf>, String> {
    use std::process::Command;
    let title = if csv {
        "Экспорт истории"
    } else if save {
        "Экспорт настроек"
    } else {
        "Импорт настроек"
    };
    let name = if csv {
        "MH-Sidebar-history.csv"
    } else {
        "MH-Sidebar-settings.json"
    };
    #[cfg(target_os = "linux")]
    let output = {
        let mut command = Command::new("zenity");
        command
            .arg("--file-selection")
            .arg(format!("--title={title}"));
        if save {
            command
                .arg("--save")
                .arg("--confirm-overwrite")
                .arg(format!("--filename={name}"));
        }
        match command.output() {
            Ok(output) => output,
            Err(_) => {
                let mut command = Command::new("kdialog");
                command.arg(if save {
                    "--getsavefilename"
                } else {
                    "--getopenfilename"
                });
                if save {
                    command.arg(name);
                }
                command
                    .output()
                    .map_err(|error| format!("Нужен zenity или kdialog: {error}"))?
            }
        }
    };
    #[cfg(target_os = "macos")]
    let output = {
        let script = if save {
            format!(
                "POSIX path of (choose file name with prompt \"{title}\" default name \"{name}\")"
            )
        } else {
            format!("POSIX path of (choose file with prompt \"{title}\")")
        };
        Command::new("osascript")
            .args(["-e", &script])
            .output()
            .map_err(|error| error.to_string())?
    };
    if !output.status.success() {
        return Ok(None);
    }
    let path = String::from_utf8(output.stdout).map_err(|error| error.to_string())?;
    Ok(Some(PathBuf::from(path.trim_end_matches(|value: char| {
        value == '\r' || value == '\n'
    }))))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exporting_to_active_file_is_rejected_even_before_first_save() {
        let path = std::env::temp_dir().join("mh-not-yet-created-settings.json");
        let name = if cfg!(windows) {
            "MH-NOT-YET-CREATED-SETTINGS.JSON"
        } else {
            "mh-not-yet-created-settings.json"
        };
        let alias = path.parent().unwrap().join(".").join(name);
        assert!(same_destination(&path, &alias));
        assert!(!same_destination(&path, &path.with_file_name("other.json")));
    }
    #[test]
    fn csv_destination_rejects_existing_settings_file() {
        let dir = std::env::temp_dir().join(format!("mh-csv-guard-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let settings = dir.join("settings.json");
        std::fs::write(&settings, b"private settings").unwrap();
        let name = if cfg!(windows) {
            "SETTINGS.JSON"
        } else {
            "settings.json"
        };
        assert!(same_destination(&settings, &dir.join(".").join(name)));
        assert!(!same_destination(&settings, &dir.join("history.csv")));
        assert_eq!(std::fs::read(&settings).unwrap(), b"private settings");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
