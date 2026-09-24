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
            .map(|p| p.to_string_lossy().to_lowercase())
    };
    matches!((resolve(a), resolve(b)), (Ok(a), Ok(b)) if a == b)
}

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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exporting_to_active_file_is_rejected_even_before_first_save() {
        let path = std::env::temp_dir().join("mh-not-yet-created-settings.json");
        let alias = path
            .parent()
            .unwrap()
            .join(".")
            .join("MH-NOT-YET-CREATED-SETTINGS.JSON");
        assert!(same_destination(&path, &alias));
        assert!(!same_destination(&path, &path.with_file_name("other.json")));
    }
    #[test]
    fn csv_destination_rejects_existing_settings_file() {
        let dir = std::env::temp_dir().join(format!("mh-csv-guard-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let settings = dir.join("settings.json");
        std::fs::write(&settings, b"private settings").unwrap();
        assert!(same_destination(
            &settings,
            &dir.join(".").join("SETTINGS.JSON")
        ));
        assert!(!same_destination(&settings, &dir.join("history.csv")));
        assert_eq!(std::fs::read(&settings).unwrap(), b"private settings");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
