//! Thumbnail decode: raw image bytes (phone `/thumb` JPEG) → gpui
//! [`RenderImage`] frames (BGRA), powering grid-tile previews.

use gpui_kit::RenderImage;

/// Longest edge the desktop asks the phone for.
pub const THUMB_MAX_DIM: u32 = 256;

/// Longest edge for full inspector previews (posters) — sharper than grid
/// thumbnails while staying phone-friendly to transfer.
pub const PREVIEW_MAX_DIM: u32 = 720;

/// Decodes `bytes` into a single-frame render image (`None` on decode
/// failure or zero-sized images). Runs on a background thread.
pub fn decode(bytes: &[u8]) -> Option<RenderImage> {
    let decoded = image::load_from_memory(bytes).ok()?;
    let rgba = decoded.to_rgba8();
    let (width, height) = rgba.dimensions();
    if width == 0 || height == 0 {
        return None;
    }
    // gpui samples BGRA; the image crate hands back RGBA.
    let mut bgra = rgba.into_raw();
    for chunk in bgra.chunks_exact_mut(4) {
        chunk.swap(0, 2);
    }
    let buffer = image::RgbaImage::from_raw(width, height, bgra)?;
    Some(RenderImage::new(vec![image::Frame::new(buffer)]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encoded_png(width: u32, height: u32) -> Vec<u8> {
        let buffer =
            image::RgbaImage::from_fn(width, height, |x, _| image::Rgba([x as u8, 40, 80, 255]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        buffer
            .write_to(&mut bytes, image::ImageFormat::Png)
            .expect("encode");
        bytes.into_inner()
    }

    #[test]
    fn decodes_png_bytes() {
        let rendered = decode(&encoded_png(4, 2)).expect("render image");
        let bytes = rendered.as_bytes(0).expect("frame");
        // BGRA: 4 px per row × 2 rows, 4 bytes each.
        assert_eq!(bytes.len(), 4 * 2 * 4);
        // First pixel's blue channel (was x=0 red) leads: B=80, G=40, R=0.
        assert_eq!(bytes[0], 80);
        assert_eq!(bytes[1], 40);
        assert_eq!(bytes[2], 0);
    }

    #[test]
    fn rejects_garbage_and_empty() {
        assert!(decode(b"not an image").is_none());
        assert!(decode(&[]).is_none());
    }
}
