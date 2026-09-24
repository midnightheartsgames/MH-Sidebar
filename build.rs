#[cfg(windows)]
use std::path::PathBuf;

#[cfg(not(windows))]
fn main() {}

#[cfg(windows)]
fn main() {
    println!("cargo:rerun-if-changed=assets/app.manifest");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let manifest_template =
        PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("assets/app.manifest");
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let manifest = out.join("mh-sidebar.manifest");
    let manifest_contents = std::fs::read_to_string(manifest_template)
        .unwrap()
        .replace("{version}", &version);
    std::fs::write(&manifest, manifest_contents).unwrap();
    let mut icon = Vec::new();
    for n in [0u16, 1, 1] {
        icon.extend(n.to_le_bytes());
    }
    icon.extend([32, 32, 0, 0]);
    icon.extend(1u16.to_le_bytes());
    icon.extend(32u16.to_le_bytes());
    icon.extend(4264u32.to_le_bytes());
    icon.extend(22u32.to_le_bytes());
    for n in [40u32, 32, 64] {
        icon.extend(n.to_le_bytes());
    }
    icon.extend(1u16.to_le_bytes());
    icon.extend(32u16.to_le_bytes());
    for n in [0u32, 4096, 0, 0, 0, 0] {
        icon.extend(n.to_le_bytes());
    }
    for y in (0..32).rev() {
        for x in 0..32 {
            let visible = [(4, 18), (13, 7), (22, 12)]
                .iter()
                .any(|(l, t)| x >= *l && x < l + 6 && y >= *t && y < 28);
            icon.extend(if visible {
                [216, 208, 63, 255]
            } else {
                [0, 0, 0, 0]
            });
        }
    }
    icon.extend([0u8; 128]);
    let icon_path = out.join("mh-sidebar.ico");
    std::fs::write(&icon_path, icon).unwrap();
    let escaped = |p: &PathBuf| p.display().to_string().replace('\\', "\\\\");
    let numeric = format!("{},0", version.replace('.', ","));
    let rc = format!(
        r#"#pragma code_page(65001)
1 ICON "{icon}"
1 24 "{manifest}"
1 VERSIONINFO
FILEVERSION {numeric}
PRODUCTVERSION {numeric}
FILEOS 0x40004
FILETYPE 0x1
BEGIN
 BLOCK "StringFileInfo"
 BEGIN
  BLOCK "040904B0"
  BEGIN
   VALUE "CompanyName", "midnightheartsgames"
   VALUE "FileDescription", "MH Sidebar — system monitor"
   VALUE "FileVersion", "{version}"
   VALUE "ProductName", "MH Sidebar"
   VALUE "ProductVersion", "{version}"
   VALUE "OriginalFilename", "MH-Sidebar.exe"
  END
 END
 BLOCK "VarFileInfo"
 BEGIN
  VALUE "Translation", 0x409, 1200
 END
END
"#,
        icon = escaped(&icon_path),
        manifest = escaped(&manifest)
    );
    let path = out.join("mh-sidebar.rc");
    std::fs::write(&path, rc).unwrap();
    embed_resource::compile(&path, embed_resource::NONE)
        .manifest_required()
        .unwrap();
}
