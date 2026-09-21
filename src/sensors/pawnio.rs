// MIT License
//
// Copyright (c) 2026 midnightheartsgames
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.
//
// ---
//
// Third-party components bundled with this software keep their own licenses:
//
// - Intel PresentMon 2.5.1 — MIT License, see assets/presentmon/LICENSE.txt
// - PawnIO modules AMDFamily17 and IntelMSR 0.2.11 — LGPL-2.1-or-later,
//   see assets/pawnio/COPYING
// - Cuprum font — SIL Open Font License 1.1, see assets/fonts/OFL.txt
//
//! Связь с драйвером PawnIO — перенесено из MH Monitoring (`crates/platform/src/pawnio.rs`).
//!
//! PawnIO — подписанный драйвер ядра, который выполняет только подписанные модули, а те отдают
//! наружу узкие вызовы вроде «прочитать такой-то MSR из белого списка». Через него читаются
//! температура и мощность CPU, недоступные из пользовательского режима.
//!
//! **С `PawnIOLib.dll` мы не линкуемся.** Драйвер распространяется под GPL-2.0 с исключением
//! только для программ, которые общаются с ним через интерфейс IOCTL устройства. Поэтому протокол
//! повторён здесь напрямую — он простой. Сам драйвер ставит его официальный установщик.

use windows_sys::Win32::Foundation::{
    CloseHandle, GENERIC_READ, GENERIC_WRITE, GetLastError, HANDLE, INVALID_HANDLE_VALUE,
    WAIT_ABANDONED, WAIT_OBJECT_0,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Threading::{
    CreateMutexW, MUTEX_MODIFY_STATE, OpenMutexW, ReleaseMutex, SYNCHRONIZATION_SYNCHRONIZE,
    WaitForSingleObject,
};

use super::wide;

/// `\Device\PawnIO` в пространстве имён NT; `GLOBALROOT` делает его доступным для `CreateFileW`.
const DEVICE_PATH: &str = r"\\?\GLOBALROOT\Device\PawnIO";

const DEVICE_TYPE: u32 = 41394;
const METHOD_BUFFERED: u32 = 0;
const FILE_ANY_ACCESS: u32 = 0;

/// Длина поля имени функции во входном буфере `IOCTL_PIO_EXECUTE_FN`.
const FN_NAME_LENGTH: usize = 32;

/// `CTL_CODE` из заголовков WDK.
const fn ctl_code(device_type: u32, function: u32, method: u32, access: u32) -> u32 {
    (device_type << 16) | (access << 14) | (function << 2) | method
}

const IOCTL_PIO_LOAD_BINARY: u32 = ctl_code(DEVICE_TYPE, 0x821, METHOD_BUFFERED, FILE_ANY_ACCESS);
const IOCTL_PIO_EXECUTE_FN: u32 = ctl_code(DEVICE_TYPE, 0x841, METHOD_BUFFERED, FILE_ANY_ACCESS);

/// Почему PawnIO недоступен или вызов не удался.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PawnIoError {
    /// Драйвер не установлен: устройства нет.
    NotInstalled,
    /// Устройство есть, но открыть его не дали. Нужен администратор.
    AccessDenied,
    /// Имя функции не помещается в 32 байта протокола.
    NameTooLong,
    /// Драйвер вернул ошибку — модуль не подошёл к этому CPU, регистр не в белом списке и т. п.
    Driver(u32),
}

/// Открытый дескриптор PawnIO с загруженным модулем. Один дескриптор — один модуль.
pub struct PawnIo {
    handle: HANDLE,
}

// Дескриптор устройства — непрозрачный номер ядра. Вызовы драйвера сериализует сам драйвер.
unsafe impl Send for PawnIo {}

impl PawnIo {
    /// Открывает драйвер и загружает в него подписанный модуль.
    pub fn open_with_module(module: &[u8]) -> Result<Self, PawnIoError> {
        let path = wide(DEVICE_PATH);
        let handle = unsafe {
            CreateFileW(
                path.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(match unsafe { GetLastError() } {
                // ERROR_FILE_NOT_FOUND и ERROR_PATH_NOT_FOUND — устройства нет.
                2 | 3 => PawnIoError::NotInstalled,
                5 => PawnIoError::AccessDenied,
                other => PawnIoError::Driver(other),
            });
        }
        let pawnio = Self { handle };
        pawnio.ioctl(IOCTL_PIO_LOAD_BINARY, module, &mut [])?;
        Ok(pawnio)
    }

    /// Вызывает функцию модуля с одним аргументом и возвращает первое записанное значение.
    pub fn call(&self, name: &str, argument: u64) -> Result<u64, PawnIoError> {
        let request = encode_execute_request(name, &[argument])?;
        let mut raw = [0u8; 8];
        let written = self.ioctl(IOCTL_PIO_EXECUTE_FN, &request, &mut raw)?;
        let mut out = [0u64; 1];
        match decode_execute_response(&raw[..written], &mut out) {
            1 => Ok(out[0]),
            // Драйвер ответил успехом, но ничего не записал — показания нет.
            _ => Err(PawnIoError::Driver(0)),
        }
    }

    fn ioctl(&self, code: u32, input: &[u8], output: &mut [u8]) -> Result<usize, PawnIoError> {
        let mut returned = 0u32;
        let ok = unsafe {
            DeviceIoControl(
                self.handle,
                code,
                input.as_ptr().cast(),
                input.len() as u32,
                if output.is_empty() {
                    std::ptr::null_mut()
                } else {
                    output.as_mut_ptr().cast()
                },
                output.len() as u32,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(match unsafe { GetLastError() } {
                5 => PawnIoError::AccessDenied,
                other => PawnIoError::Driver(other),
            });
        }
        Ok(returned as usize)
    }
}

impl Drop for PawnIo {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.handle) };
    }
}

/// Входной буфер `IOCTL_PIO_EXECUTE_FN`: имя функции в 32 байтах, дополненных нулями, затем
/// аргументы как `u64` в порядке x64.
fn encode_execute_request(name: &str, input: &[u64]) -> Result<Vec<u8>, PawnIoError> {
    // Последний байт поля обязан остаться нулём — драйвер читает имя как C-строку.
    if name.len() >= FN_NAME_LENGTH {
        return Err(PawnIoError::NameTooLong);
    }
    let mut request = vec![0u8; FN_NAME_LENGTH + input.len() * 8];
    request[..name.len()].copy_from_slice(name.as_bytes());
    for (index, value) in input.iter().enumerate() {
        let offset = FN_NAME_LENGTH + index * 8;
        request[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    Ok(request)
}

fn decode_execute_response(raw: &[u8], out: &mut [u64]) -> usize {
    let (words, _) = raw.as_chunks::<8>();
    let count = words.len().min(out.len());
    for (slot, word) in out.iter_mut().zip(words) {
        *slot = u64::from_le_bytes(*word);
    }
    count
}

/// Общесистемный мьютекс доступа к конфигурационному пространству PCI.
///
/// Пара регистров «индекс/данные», через которую читается SMN у AMD, одна на всю систему. Если
/// HWiNFO или LibreHardwareMonitor запущены одновременно, без этого мьютекса наш индекс может
/// перезаписать чужой, и обе программы прочитают мусор. Имя общее: так его называют и они.
pub struct PciAccessLock {
    handle: HANDLE,
}

unsafe impl Send for PciAccessLock {}

impl PciAccessLock {
    pub fn open() -> Result<Self, PawnIoError> {
        let name = wide(r"Global\Access_PCI");
        // Another monitor (HWiNFO, LibreHardwareMonitor, Fan Control) may already own the mutex
        // with a restrictive DACL: `CreateMutexW` asks for full access and gets ACCESS_DENIED
        // there. Waiting and releasing need only these two rights.
        let mut handle = unsafe {
            OpenMutexW(
                SYNCHRONIZATION_SYNCHRONIZE | MUTEX_MODIFY_STATE,
                0,
                name.as_ptr(),
            )
        };
        if handle.is_null() {
            handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        }
        if handle.is_null() {
            return Err(PawnIoError::Driver(unsafe { GetLastError() }));
        }
        Ok(Self { handle })
    }

    /// Выполняет `action` под мьютексом. `None`, если мьютекс не удалось взять за `timeout_ms`.
    pub fn with<T>(&self, timeout_ms: u32, action: impl FnOnce() -> T) -> Option<T> {
        let wait = unsafe { WaitForSingleObject(self.handle, timeout_ms) };
        // Брошенный мьютекс — владелец умер, не освободив его. Он теперь наш, и это не ошибка.
        if wait != WAIT_OBJECT_0 && wait != WAIT_ABANDONED {
            return None;
        }
        let result = action();
        unsafe { ReleaseMutex(self.handle) };
        Some(result)
    }
}

impl Drop for PciAccessLock {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.handle) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Коды сверены с `CTL_CODE(41394, 0x821/0x841, METHOD_BUFFERED, FILE_ANY_ACCESS)` из
    /// `pawnio_um.h`.
    #[test]
    fn ioctl_codes_match_the_driver_header() {
        assert_eq!(IOCTL_PIO_LOAD_BINARY, (41394 << 16) | (0x821 << 2));
        assert_eq!(IOCTL_PIO_EXECUTE_FN, 0xA1B2_2104);
    }

    #[test]
    fn a_request_is_a_padded_name_followed_by_arguments() {
        let request = encode_execute_request("ioctl_read_msr", &[0xC001_029B]).unwrap();
        assert_eq!(request.len(), 32 + 8);
        assert_eq!(&request[..14], b"ioctl_read_msr");
        assert!(request[14..32].iter().all(|&byte| byte == 0));
        assert_eq!(
            u64::from_le_bytes(request[32..40].try_into().unwrap()),
            0xC001_029B
        );
    }

    #[test]
    fn a_name_must_leave_room_for_the_terminator() {
        assert!(encode_execute_request(&"x".repeat(31), &[]).is_ok());
        assert_eq!(
            encode_execute_request(&"x".repeat(32), &[]),
            Err(PawnIoError::NameTooLong)
        );
    }

    #[test]
    fn a_short_output_buffer_is_not_overrun() {
        let raw = [1u8; 24];
        let mut out = [0u64; 1];
        assert_eq!(decode_execute_response(&raw, &mut out), 1);
    }
}
