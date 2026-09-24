use super::{from_wide, wide};
use windows_sys::Win32::{
    NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2, MIB_IF_TYPE_LOOPBACK},
    Storage::FileSystem::GetVolumeNameForVolumeMountPointW,
};

pub fn volume_id(mount: &str) -> String {
    let mut buffer = [0u16; 128];
    if unsafe {
        GetVolumeNameForVolumeMountPointW(
            wide(mount).as_ptr(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
        )
    } != 0
    {
        format!("volume:{}", from_wide(&buffer).to_lowercase())
    } else {
        format!("unstable:volume:{}", mount.to_lowercase())
    }
}

pub struct NetworkDevice {
    pub id: String,
    pub name: String,
    pub received: u64,
    pub transmitted: u64,
}

pub fn networks() -> Result<Vec<NetworkDevice>, String> {
    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    let result = unsafe { GetIfTable2(&mut table) };
    if result != 0 {
        return Err(format!(
            "GetIfTable2: {}",
            std::io::Error::from_raw_os_error(result as i32)
        ));
    }
    if table.is_null() {
        return Err("GetIfTable2: пустая таблица".into());
    }
    struct OwnedTable(*mut MIB_IF_TABLE2);
    impl Drop for OwnedTable {
        fn drop(&mut self) {
            unsafe {
                FreeMibTable(self.0.cast());
            }
        }
    }
    let table = OwnedTable(table);
    let rows = unsafe {
        std::slice::from_raw_parts((*table.0).Table.as_ptr(), (*table.0).NumEntries as usize)
    };
    Ok(rows.iter().filter_map(network_device).collect())
}

fn network_device(
    row: &windows_sys::Win32::NetworkManagement::IpHelper::MIB_IF_ROW2,
) -> Option<NetworkDevice> {
    use windows_sys::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    if row.Type == MIB_IF_TYPE_LOOPBACK
        || row.OperStatus != IfOperStatusUp
        || row.InterfaceAndOperStatusFlags._bitfield & 2 != 0
    {
        return None;
    }
    let guid = row.InterfaceGuid;
    let tail: String = guid.data4.iter().map(|b| format!("{b:02x}")).collect();
    let id = if guid.data1 == 0 && guid.data2 == 0 && guid.data3 == 0 && guid.data4 == [0; 8] {
        format!("unstable:network:index:{}", row.InterfaceIndex)
    } else {
        format!(
            "network:{:08x}-{:04x}-{:04x}-{tail}",
            guid.data1, guid.data2, guid.data3
        )
    };
    Some(NetworkDevice {
        id,
        name: from_wide(&row.Alias),
        received: row.InOctets,
        transmitted: row.OutOctets,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::NetworkManagement::{
        IpHelper::MIB_IF_ROW2,
        Ndis::{IfOperStatusDown, IfOperStatusUp},
    };
    #[test]
    fn renamed_network_keeps_guid_and_driver_filter_rows_are_not_devices() {
        let mut row = MIB_IF_ROW2 {
            Type: 6,
            OperStatus: IfOperStatusUp,
            ..Default::default()
        };
        row.InterfaceGuid.data1 = 0x12345678;
        row.Alias[..4].copy_from_slice(&[b'O' as u16, b'l' as u16, b'd' as u16, 0]);
        row.InOctets = 1024;
        let old = network_device(&row).unwrap();
        row.Alias[..4].copy_from_slice(&[b'N' as u16, b'e' as u16, b'w' as u16, 0]);
        let new = network_device(&row).unwrap();
        assert_eq!(old.id, new.id);
        assert_ne!(old.name, new.name);
        assert_eq!(new.received, 1024);
        row.InterfaceAndOperStatusFlags._bitfield = 2;
        assert!(network_device(&row).is_none());
        row.InterfaceAndOperStatusFlags._bitfield = 0;
        row.OperStatus = IfOperStatusDown;
        assert!(network_device(&row).is_none());
    }
}
