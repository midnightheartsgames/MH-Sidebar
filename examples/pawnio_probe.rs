mod shim {
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }
    #[path = "../../src/sensors/pawnio.rs"]
    pub mod pawnio;
}
use shim::pawnio::{PawnIo, PciAccessLock};

fn main() {
    let module = include_bytes!("../assets/pawnio/AMDFamily17.bin");
    let pawnio = match PawnIo::open_with_module(module) {
        Ok(p) => {
            println!("open+load: ok");
            p
        }
        Err(e) => return println!("open+load: {e:?}"),
    };
    println!(
        "read_msr PWR_UNIT: {:?}",
        pawnio.call("ioctl_read_msr", 0xC001_0299)
    );
    println!(
        "read_msr PKG_ENERGY: {:?}",
        pawnio.call("ioctl_read_msr", 0xC001_029B)
    );
    println!(
        "read_smn TCTL: {:?}",
        pawnio.call("ioctl_read_smn", 0x0005_9800)
    );
    match PciAccessLock::open() {
        Ok(lock) => println!("pci lock: {:?}", lock.with(50, || ())),
        Err(e) => println!("pci lock: {e:?}"),
    }
}
