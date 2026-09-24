fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

#[path = "../../src/sensors/pawnio.rs"]
pub mod pawnio;
