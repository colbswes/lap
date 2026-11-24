/**
 * Pet recognition: Apple Vision finds cats and dogs; an identity embedding of
 * each animal crop tells individual pets apart (clustered like faces).
 */

/// Species reported by the animal detector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnimalKind {
    Cat,
    Dog,
}

impl AnimalKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AnimalKind::Cat => "cat",
            AnimalKind::Dog => "dog",
        }
    }
}

/// Detected animal, box normalized to the image with a top-left origin.
#[derive(Debug, Clone, Copy)]
pub struct AnimalBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub kind: AnimalKind,
    pub confidence: f32,
}

pub const IMAGENET_MEAN: [f32; 3] = [0.485, 0.456, 0.406];
pub const IMAGENET_STD: [f32; 3] = [0.229, 0.224, 0.225];

/// Square crop around a box (pixels), padded 10% per side, with black outside the
/// image, resized to `size` x `size`. Matches the crops the pet model was evaluated on.
pub fn square_crop(img: &image::DynamicImage, bbox: (f32, f32, f32, f32), size: u32) -> image::RgbImage {
    let (x, y, w, h) = bbox;
    let side = (w.max(h) * 1.2).max(1.0);
    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
    let (left, top) = ((cx - side / 2.0).round() as i64, (cy - side / 2.0).round() as i64);
    let side_px = side.round().max(1.0) as u32;
    let mut canvas = image::RgbImage::new(side_px, side_px);
    let rgb = img.to_rgb8();
    let (iw, ih) = (rgb.width() as i64, rgb.height() as i64);
    let (x0, y0) = (left.max(0), top.max(0));
    let (x1, y1) = ((left + side_px as i64).min(iw), (top + side_px as i64).min(ih));
    if x1 > x0 && y1 > y0 {
        let region = image::imageops::crop_imm(&rgb, x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32).to_image();
        image::imageops::replace(&mut canvas, &region, x0 - left, y0 - top);
    }
    image::imageops::resize(&canvas, size, size, image::imageops::FilterType::CatmullRom)
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn lap_vision_detect_animals(
        data: *const u8,
        len: usize,
        out: *mut f32,
        max_results: std::ffi::c_int,
        error_out: *mut *mut std::ffi::c_char,
    ) -> std::ffi::c_int;
    fn lap_vision_free_string(ptr: *mut std::ffi::c_char);
}

const MAX_ANIMALS: usize = 32;

/// Detect cats and dogs in an encoded image with Apple Vision.
#[cfg(target_os = "macos")]
pub fn detect_animals(image_bytes: &[u8]) -> Result<Vec<AnimalBox>, String> {
    let mut out = vec![0f32; MAX_ANIMALS * 6];
    let mut error: *mut std::ffi::c_char = std::ptr::null_mut();
    let count = unsafe {
        lap_vision_detect_animals(
            image_bytes.as_ptr(),
            image_bytes.len(),
            out.as_mut_ptr(),
            MAX_ANIMALS as std::ffi::c_int,
            &mut error,
        )
    };
    if count < 0 {
        let message = if error.is_null() {
            "Vision animal detection failed".to_string()
        } else {
            let text = unsafe { std::ffi::CStr::from_ptr(error) }.to_string_lossy().into_owned();
            unsafe { lap_vision_free_string(error) };
            text
        };
        return Err(message);
    }
    Ok(out
        .chunks_exact(6)
        .take(count as usize)
        .map(|r| AnimalBox {
            x: r[0],
            y: r[1],
            width: r[2],
            height: r[3],
            kind: if r[4] < 0.5 { AnimalKind::Cat } else { AnimalKind::Dog },
            confidence: r[5],
        })
        .collect())
}

/// Animal detection needs Apple Vision; other platforms find no animals.
#[cfg(not(target_os = "macos"))]
pub fn detect_animals(_image_bytes: &[u8]) -> Result<Vec<AnimalBox>, String> {
    Ok(Vec::new())
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    /// `PET_IMAGE=/path/to/pet.jpg cargo test --bin Lap vision_finds -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn vision_finds_animals_in_a_pet_photo() {
        let path = std::env::var("PET_IMAGE").unwrap();
        let bytes = std::fs::read(path).unwrap();
        let animals = detect_animals(&bytes).unwrap();
        println!("{:?}", animals);
        assert!(!animals.is_empty());
    }
}
