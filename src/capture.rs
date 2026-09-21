//! Debug-build visual QA hook: captures only this application's egui framebuffer.
//! Opt in by setting MH_SIDEBAR_CAPTURE_DIR to a local output directory.
pub fn frame(ui: &eframe::egui::Ui, name: &str, now: f64) {
    use eframe::egui::{self, ViewportCommand};
    let Some(dir) = std::env::var_os("MH_SIDEBAR_CAPTURE_DIR") else {
        return;
    };
    let ctx = ui.ctx();
    let requested = egui::Id::new(("capture", name));
    if now > 3. && !ctx.data(|d| d.get_temp::<bool>(requested).unwrap_or(false)) {
        ctx.data_mut(|d| d.insert_temp(requested, true));
        ctx.send_viewport_cmd(ViewportCommand::Screenshot(Default::default()));
    }
    ctx.input(|input| {
        for event in &input.events {
            if let egui::Event::Screenshot { image, .. } = event {
                let path = std::path::PathBuf::from(&dir).join(format!("{name}.png"));
                let rgba: Vec<u8> = image
                    .pixels
                    .iter()
                    .flat_map(|pixel| pixel.to_array())
                    .collect();
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Err(e) = image::save_buffer(
                    &path,
                    &rgba,
                    image.width() as u32,
                    image.height() as u32,
                    image::ColorType::Rgba8,
                ) {
                    eprintln!("capture: {e}");
                }
            }
        }
    });
}

/// egui 0.36 does not process Screenshot actions for immediate child viewports.
/// Their GL surface is still current immediately after show_viewport_immediate.
pub fn immediate(ctx: &eframe::egui::Context, gl: &eframe::glow::Context, now: f64) {
    use eframe::{
        egui,
        glow::{self, HasContext},
    };
    let Some(dir) = std::env::var_os("MH_SIDEBAR_CAPTURE_DIR") else {
        return;
    };
    let id = egui::Id::new("settings-captured");
    if now < 4. || ctx.data(|d| d.get_temp::<bool>(id).unwrap_or(false)) {
        return;
    }
    let mut viewport = [0; 4];
    unsafe {
        gl.get_parameter_i32_slice(glow::VIEWPORT, &mut viewport);
    }
    let (w, h) = (viewport[2], viewport[3]);
    if w <= 0 || h <= 0 {
        return;
    }
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    unsafe {
        gl.read_buffer(glow::FRONT);
        gl.read_pixels(
            0,
            0,
            w,
            h,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelPackData::Slice(Some(&mut rgba)),
        );
        gl.read_buffer(glow::BACK);
    }
    let mut image = image::RgbaImage::from_raw(w as u32, h as u32, rgba).unwrap();
    image::imageops::flip_vertical_in_place(&mut image);
    let dir = std::path::PathBuf::from(dir);
    let _ = std::fs::create_dir_all(&dir);
    if image.save(dir.join("settings.png")).is_ok() {
        ctx.data_mut(|d| d.insert_temp(id, true));
    }
}
