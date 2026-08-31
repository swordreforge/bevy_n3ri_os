#[cfg(feature = "capture")]
use crate::capture::CapturedFrame;
#[cfg(feature = "capture")]
use base64::Engine;
#[cfg(feature = "capture")]
use image::imageops::FilterType;

#[cfg(feature = "capture")]
const MAX_DIMENSION: u32 = 512;
#[cfg(feature = "capture")]
const JPEG_QUALITY: u8 = 70;

#[cfg(feature = "capture")]
pub struct VisionSnapshot {
    pub base64: String,
}

#[cfg(feature = "capture")]
pub fn encode_frame(frame: &CapturedFrame) -> Option<VisionSnapshot> {
    let rgba = image::RgbaImage::from_raw(frame.width, frame.height, frame.data.clone())?;
    let _ = std::fs::write("/tmp/lv2_raw.rgba", &frame.data);
    let (w, h) = if frame.width > frame.height {
        let scale = MAX_DIMENSION as f32 / frame.width as f32;
        (MAX_DIMENSION, (frame.height as f32 * scale) as u32)
    } else {
        let scale = MAX_DIMENSION as f32 / frame.height as f32;
        ((frame.width as f32 * scale) as u32, MAX_DIMENSION)
    };

    let scaled = image::DynamicImage::ImageRgba8(rgba).resize(w, h, FilterType::Lanczos3);
    let _ = scaled.save("/tmp/lv2_scaled.png");

    let mut buf = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, JPEG_QUALITY);
    encoder.encode_image(&scaled).ok()?;

    let _ = std::fs::write("/tmp/lv2_encode.jpg", &buf);

    let base64 = format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&buf)
    );

    Some(VisionSnapshot { base64 })
}
