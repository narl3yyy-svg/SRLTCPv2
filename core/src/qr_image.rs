//! QR code image generation for desktop/mobile UI.

use base64::Engine;
use image::Luma;
use qrcode::QrCode;

/// Render a QR payload as a base64-encoded PNG data URL.
pub fn qr_png_data_url(payload: &str) -> Result<String, String> {
    let qr = QrCode::new(payload.as_bytes()).map_err(|e| e.to_string())?;
    let image = qr.render::<Luma<u8>>().quiet_zone(true).min_dimensions(200, 200).build();
    let mut bytes = Vec::new();
    image
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .map_err(|e| e.to_string())?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(format!("data:image/png;base64,{b64}"))
}