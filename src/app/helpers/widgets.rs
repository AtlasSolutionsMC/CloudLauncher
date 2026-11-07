use super::super::*;

pub(in crate::app) fn toolbar_popup_should_close(
    ctx: &egui::Context,
    popup_rect: egui::Rect,
) -> bool {
    ctx.input(|input| {
        input.key_pressed(egui::Key::Escape)
            || (input.pointer.any_pressed()
                && input
                    .pointer
                    .interact_pos()
                    .is_some_and(|position| !popup_rect.contains(position)))
    })
}

pub(in crate::app) fn icon_button(
    ui: &mut egui::Ui,
    texture: &TextureHandle,
    tint: Option<Color32>,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(58.0, 31.0), Sense::click());
    let fill = if response.hovered() {
        Color32::from_rgb(102, 108, 110)
    } else {
        Color32::from_rgb(73, 78, 80)
    };
    ui.painter().rect_filled(rect, 2.0, fill);
    ui.painter().rect_stroke(
        rect,
        2.0,
        Stroke::new(1.0, Color32::from_white_alpha(24)),
        egui::StrokeKind::Inside,
    );
    let size = Vec2::new(22.0, 22.0);
    let icon_rect = Rect::from_center_size(rect.center(), size);
    ui.painter().image(
        texture.id(),
        icon_rect,
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        tint.unwrap_or(Color32::WHITE),
    );
    response
}

pub(in crate::app) fn cancel_icon_button(
    ui: &mut egui::Ui,
    tint: Option<Color32>,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(58.0, 31.0), Sense::click());
    let fill = if response.hovered() {
        Color32::from_rgb(102, 108, 110)
    } else {
        Color32::from_rgb(73, 78, 80)
    };
    ui.painter().rect_filled(rect, 2.0, fill);
    ui.painter().rect_stroke(
        rect,
        2.0,
        Stroke::new(1.0, Color32::from_white_alpha(24)),
        egui::StrokeKind::Inside,
    );
    let center = rect.center();
    let color = tint.unwrap_or(Color32::WHITE);
    ui.painter().line_segment(
        [center + Vec2::new(-6.0, -6.0), center + Vec2::new(6.0, 6.0)],
        Stroke::new(2.5, color),
    );
    ui.painter().line_segment(
        [center + Vec2::new(6.0, -6.0), center + Vec2::new(-6.0, 6.0)],
        Stroke::new(2.5, color),
    );
    response
}

pub(in crate::app) fn load_texture(
    ctx: &Context,
    name: &str,
    bytes: &[u8],
    recolor: bool,
) -> TextureHandle {
    load_texture_from_bytes(ctx, name, bytes, recolor).expect("embedded launcher image is valid")
}

pub(in crate::app) fn load_texture_from_path(
    ctx: &Context,
    name: &str,
    path: &Path,
) -> Result<TextureHandle, String> {
    let metadata = fs::metadata(path).map_err(|error| {
        tr("rust.status.background.path-error")
            .replace("%0", &path.display().to_string())
            .replace("%1", &error.to_string())
    })?;
    if metadata.len() > 64 * 1024 * 1024 {
        return Err(tr("rust.status.background.file-large").to_owned());
    }
    let mut reader = image::ImageReader::open(path)
        .map_err(|error| {
            tr("rust.status.background.path-error")
                .replace("%0", &path.display().to_string())
                .replace("%1", &error.to_string())
        })?
        .with_guessed_format()
        .map_err(|error| {
            tr("rust.status.background.path-error")
                .replace("%0", &path.display().to_string())
                .replace("%1", &error.to_string())
        })?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let decoded = reader.decode().map_err(|error| {
        tr("rust.status.background.decode-error")
            .replace("%0", &path.display().to_string())
            .replace("%1", &error.to_string())
    })?;
    texture_from_decoded_image(ctx, name, decoded, false)
}

pub(in crate::app) fn load_texture_from_bytes(
    ctx: &Context,
    name: &str,
    bytes: &[u8],
    recolor: bool,
) -> Result<TextureHandle, String> {
    let decoded = image::load_from_memory(bytes).map_err(|error| {
        tr("rust.status.background.decode-bytes-error").replace("%0", &error.to_string())
    })?;
    texture_from_decoded_image(ctx, name, decoded, recolor)
}

pub(in crate::app) fn texture_from_decoded_image(
    ctx: &Context,
    name: &str,
    decoded: image::DynamicImage,
    recolor: bool,
) -> Result<TextureHandle, String> {
    if u64::from(decoded.width()) * u64::from(decoded.height()) > 32_000_000 {
        return Err(tr("rust.status.background.dimensions-large").to_owned());
    }
    let mut rgba = decoded.to_rgba8();
    if recolor {
        for pixel in rgba.pixels_mut() {
            if pixel[3] > 0 {
                pixel[0] = 226;
                pixel[1] = 229;
                pixel[2] = 231;
            }
        }
    }
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    );
    Ok(ctx.load_texture(name, image, TextureOptions::LINEAR))
}

pub(in crate::app) fn load_optional_texture(
    ctx: &Context,
    name: &str,
    bytes: &[u8],
) -> Option<TextureHandle> {
    if bytes.len() > 8 * 1024 * 1024 {
        return None;
    }
    let dimensions = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()?;
    if u64::from(dimensions.0).saturating_mul(u64::from(dimensions.1)) > 16_777_216 {
        return None;
    }
    let rgba = image::load_from_memory(bytes).ok()?.to_rgba8();
    let color_image = egui::ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    );
    Some(ctx.load_texture(name, color_image, TextureOptions::LINEAR))
}

pub(in crate::app) fn cover_uv(image_size: Vec2, view_size: Vec2) -> Rect {
    let image_ratio = image_size.x / image_size.y;
    let view_ratio = view_size.x / view_size.y.max(1.0);
    if image_ratio > view_ratio {
        let visible = view_ratio / image_ratio;
        let offset = (1.0 - visible) * 0.5;
        Rect::from_min_max(Pos2::new(offset, 0.0), Pos2::new(1.0 - offset, 1.0))
    } else {
        let visible = image_ratio / view_ratio;
        let offset = (1.0 - visible) * 0.5;
        Rect::from_min_max(Pos2::new(0.0, offset), Pos2::new(1.0, 1.0 - offset))
    }
}

pub(crate) fn tr(key: &'static str) -> &'static str {
    EN_US
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .find_map(|line| {
            let (candidate, value) = line.split_once('=')?;
            (candidate.trim() == key).then_some(value.trim())
        })
        .unwrap_or(key)
}

pub(crate) fn tr_dynamic(key: &str) -> String {
    EN_US
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .find_map(|line| {
            let (candidate, value) = line.split_once('=')?;
            (candidate.trim() == key).then_some(value.trim())
        })
        .unwrap_or(key)
        .to_owned()
}
