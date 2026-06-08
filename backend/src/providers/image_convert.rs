use base64::Engine;
use std::process::Command;

/// Convert base64-encoded image to JPEG base64.
/// Handles HEIC/HEIF by shelling out to `sips` (macOS) or `convert` (ImageMagick).
/// PNG/JPEG are re-encoded to JPEG via the `image` crate.
pub fn ensure_jpeg_base64(input_b64: &str) -> Result<String, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(input_b64)
        .map_err(|e| format!("base64 decode error: {e}"))?;

    if is_heic(&bytes) {
        let jpeg_bytes = heic_to_jpeg_bytes(&bytes)?;
        return convert_with_image_crate(&jpeg_bytes);
    }

    // JPEG, PNG, WebP, BMP — decode, resize, re-encode as JPEG
    convert_with_image_crate(&bytes)
}

fn is_heic(bytes: &[u8]) -> bool {
    // HEIF/HEIC: check for "ftyp" box at offset 4
    bytes.len() > 12 && &bytes[4..8] == b"ftyp"
}

fn heic_to_jpeg_bytes(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let tmp_dir = std::env::temp_dir();
    let input_path = tmp_dir.join(format!("ft_input_{}.heic", std::process::id()));
    let output_path = tmp_dir.join(format!("ft_output_{}.jpg", std::process::id()));

    std::fs::write(&input_path, bytes)
        .map_err(|e| format!("failed to write temp heic: {e}"))?;

    // Try sips (macOS)
    let result = Command::new("sips")
        .args([
            "-s", "format", "jpeg",
            "-s", "formatOptions", "80",
            input_path.to_str().unwrap(),
            "--out",
            output_path.to_str().unwrap(),
        ])
        .output();

    let success = match result {
        Ok(out) => out.status.success(),
        Err(_) => {
            // Fallback: try ImageMagick convert
            Command::new("convert")
                .args([
                    input_path.to_str().unwrap(),
                    "-quality", "80",
                    output_path.to_str().unwrap(),
                ])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        }
    };

    let _ = std::fs::remove_file(&input_path);

    if !success {
        let _ = std::fs::remove_file(&output_path);
        return Err("HEIC conversion failed: neither sips nor convert available".into());
    }

    let jpeg_bytes = std::fs::read(&output_path)
        .map_err(|e| format!("failed to read converted jpeg: {e}"))?;
    let _ = std::fs::remove_file(&output_path);

    Ok(jpeg_bytes)
}

fn convert_with_image_crate(bytes: &[u8]) -> Result<String, String> {
    let img = image::load_from_memory(bytes)
        .map_err(|e| format!("image decode error: {e}"))?;

    let mut jpeg_buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut jpeg_buf, image::ImageFormat::Jpeg)
        .map_err(|e| format!("jpeg encode error: {e}"))?;

    Ok(base64::engine::general_purpose::STANDARD.encode(jpeg_buf.into_inner()))
}
