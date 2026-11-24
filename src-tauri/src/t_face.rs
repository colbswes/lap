/**
 * Face Recognition module
 * Handles face detection (RetinaFace) and embedding (MobileFaceNet) using ONNX Runtime.
 */
use crate::{t_cluster, t_common, t_sqlite};
use image::DynamicImage;
use ndarray::Array;
use ort::{
    inputs,
    session::{Session, builder::GraphOptimizationLevel},
    value::Value,
};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};

// cancellation token for face indexing
#[derive(Clone)]
pub struct FaceIndexCancellation(pub Arc<Mutex<bool>>);

// detailed status for face indexing
#[derive(Clone)]
pub struct FaceIndexingStatus(pub Arc<Mutex<bool>>);

// face indexing progress
#[derive(Clone, serde::Serialize)]
pub struct FaceIndexProgress {
    pub current: usize,
    pub total: usize,
    pub faces_found: usize,
    pub phase: String,
}

#[derive(Clone)]
pub struct FaceIndexProgressState(pub Arc<Mutex<FaceIndexProgress>>);

// face stats
#[derive(Clone, serde::Serialize)]
pub struct FaceStats {
    pub total: usize,
    pub processed: usize,
    pub unprocessed: usize,
    pub faces: usize,
}

/// Detected face bounding box and landmarks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaceBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub confidence: f32,
    pub landmarks: Option<Vec<(f32, f32)>>, // 5 facial landmarks
}

/// Face with embedding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaceData {
    pub bbox: FaceBox,
    pub embedding: Vec<f32>,
    /// Small, strongly turned, or uncertain face (see `is_low_quality`).
    pub low_quality: bool,
}

/// Faces whose embeddings are unreliable: small in the decoded image, turned
/// strongly sideways (judged from the 5 landmarks), or detected with low confidence.
pub(crate) fn is_low_quality(face: &FaceBox) -> bool {
    if face.width.min(face.height) < t_common::LOW_QUALITY_MIN_FACE_PX
        || face.confidence < t_common::LOW_QUALITY_MIN_CONFIDENCE
    {
        return true;
    }
    let Some(lm) = face.landmarks.as_deref().filter(|lm| lm.len() == 5) else {
        return false;
    };
    let (ex, ey) = (lm[1].0 - lm[0].0, lm[1].1 - lm[0].1);
    let eye_dist = (ex * ex + ey * ey).sqrt();
    if eye_dist <= f32::EPSILON {
        return true;
    }
    // Nose offset from the eye midpoint, measured along the eye line.
    let (mx, my) = ((lm[0].0 + lm[1].0) / 2.0, (lm[0].1 + lm[1].1) / 2.0);
    let yaw = ((lm[2].0 - mx) * ex + (lm[2].1 - my) * ey) / (eye_dist * eye_dist);
    yaw.abs() > t_common::LOW_QUALITY_MAX_YAW
}

struct Anchor {
    cx: f32,
    cy: f32,
}

pub struct FaceEngine {
    detection_model: Option<Session>, // RetinaFace
    embedding_model: Option<Session>, // MobileFaceNet
}

impl FaceEngine {
    pub fn new() -> Self {
        Self {
            detection_model: None,
            embedding_model: None,
        }
    }

    pub fn load_models(&mut self, app: &AppHandle) -> Result<(), String> {
        if self.detection_model.is_some() {
            return Ok(());
        }

        // Resolve paths
        let resource_dir = app
            .path()
            .resolve("models", tauri::path::BaseDirectory::Resource)
            .map_err(|e| format!("Failed to resolve resource path: {}", e))?;

        let detection_model_path = resource_dir.join(t_common::DETECTION_MODEL);
        let embedding_model_path = resource_dir.join(t_common::EMBEDDING_MODEL);

        // Check if models exist
        if !detection_model_path.exists() {
            return Err(format!(
                "Detection model not found at {:?}",
                detection_model_path
            ));
        }
        if !embedding_model_path.exists() {
            return Err(format!(
                "Embedding model not found at {:?}",
                embedding_model_path
            ));
        }

        // Load Detection Model (RetinaFace)
        let detection_model = Session::builder()
            .map_err(|e| e.to_string())?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| e.to_string())?
            .with_intra_threads(4)
            .map_err(|e| e.to_string())?
            .commit_from_file(&detection_model_path)
            .map_err(|e| format!("Failed to load detection model: {}", e))?;

        self.detection_model = Some(detection_model);

        // Load Embedding Model (MobileFaceNet)
        let embedding_model = Session::builder()
            .map_err(|e| e.to_string())?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| e.to_string())?
            .with_intra_threads(4)
            .map_err(|e| e.to_string())?
            .commit_from_file(&embedding_model_path)
            .map_err(|e| format!("Failed to load embedding model: {}", e))?;

        self.embedding_model = Some(embedding_model);

        Ok(())
    }

    pub fn is_loaded(&self) -> bool {
        self.detection_model.is_some() && self.embedding_model.is_some()
    }

    /// Detect faces implementation (from DynamicImage)
    fn detect_faces(&mut self, img: &DynamicImage) -> Result<Vec<FaceBox>, String> {
        let original_width = img.width() as f32;
        let original_height = img.height() as f32;

        // RetinaFace typically expects 640x640 input, but works with any size divisible by 32 (stride 32).
        // Optimization: For small images (like thumbnails ~512px), use their native size slightly rounded up.
        // For large images, downscale to 640px max dimension.
        let max_dim = original_width.max(original_height);
        let target_size = if max_dim < 640.0 {
            // Round up to nearest multiple of 32
            ((max_dim as u32 + 31) / 32) * 32
        } else {
            640
        };
        // Resize preserving aspect ratio (Letterbox)
        // Use max dimension to fit within target
        let scale = (target_size as f32) / original_width.max(original_height);
        // Use round() to minimize truncation error
        let new_w = (original_width * scale).round() as u32;
        let new_h = (original_height * scale).round() as u32;

        let rgb_buf; // Owned buffer if needed
        let rgb_img = if new_w == img.width() && new_h == img.height() {
            // Optimization: Skip resize if unnecessary
            if let Some(buf) = img.as_rgb8() {
                buf
            } else {
                rgb_buf = img.to_rgb8();
                &rgb_buf
            }
        } else {
            rgb_buf = img
                .resize_exact(new_w, new_h, image::imageops::FilterType::Triangle)
                .into_rgb8();
            &rgb_buf
        };

        // Standard InsightFace/RetinaFace preprocessing aligns to Top-Left (0,0)

        // Normalize: (pixel - 127.5) / 128.0
        // Initialize with zeros (padding)
        let mut array = Array::zeros((1, 3, target_size as usize, target_size as usize));

        if let Some(slice) = array.as_slice_mut() {
            let area = (target_size as usize) * (target_size as usize);
            let offset_r = 0; // RGB planes, matching InsightFace (blobFromImage swapRB=True)
            let offset_g = area;
            let offset_b = area * 2;
            let target_w = target_size as usize;

            for (x, y, pixel) in rgb_img.enumerate_pixels() {
                let r = (pixel[0] as f32 - 127.5) / 128.0;
                let g = (pixel[1] as f32 - 127.5) / 128.0;
                let b = (pixel[2] as f32 - 127.5) / 128.0;

                let idx = (y as usize) * target_w + (x as usize);

                slice[offset_b + idx] = b;
                slice[offset_g + idx] = g;
                slice[offset_r + idx] = r;
            }
        } else {
            // Fallback if array is not contiguous (should not happen with default init)
            for (x, y, pixel) in rgb_img.enumerate_pixels() {
                let r = (pixel[0] as f32 - 127.5) / 128.0;
                let g = (pixel[1] as f32 - 127.5) / 128.0;
                let b = (pixel[2] as f32 - 127.5) / 128.0;

                array[[0, 0, y as usize, x as usize]] = r; // Red
                array[[0, 1, y as usize, x as usize]] = g; // Green
                array[[0, 2, y as usize, x as usize]] = b; // Blue
            }
        }

        let input_value = Value::from_array(array).map_err(|e| e.to_string())?;

        // Use block scope to ensure outputs is dropped before calling nms
        let mut faces = {
            let outputs = self
                .detection_model
                .as_mut()
                .unwrap()
                .run(inputs!["input.1" => input_value])
                .map_err(|e| format!("Detection inference error: {}", e))?;

            let mut all_detections = Vec::new();
            let strides = [8, 16, 32];
            let min_sizes = [[16, 32], [64, 128], [256, 512]]; // Standard RetinaFace config

            // Map output indices based on observation
            // Scores, Boxes, Landmarks indices per stride
            let indices = [
                (0, 3, 6), // Stride 8
                (1, 4, 7), // Stride 16
                (2, 5, 8), // Stride 32
            ];

            let confidence_threshold = t_common::MIN_CONFIDENCE;

            for (i, &stride) in strides.iter().enumerate() {
                let (score_idx, box_idx, kps_idx) = indices[i];

                let scores_tensor = &outputs[score_idx];
                let boxes_tensor = &outputs[box_idx];

                let (_, scores_data) = scores_tensor
                    .try_extract_tensor::<f32>()
                    .map_err(|e| format!("Failed stride {} scores: {}", stride, e))?;
                let (_, boxes_data) = boxes_tensor
                    .try_extract_tensor::<f32>()
                    .map_err(|e| format!("Failed stride {} boxes: {}", stride, e))?;
                // Landmarks are optional: a model without the kps head still yields boxes.
                let kps_data = if kps_idx < outputs.len() {
                    outputs[kps_idx]
                        .try_extract_tensor::<f32>()
                        .ok()
                        .map(|(_, data)| data)
                } else {
                    None
                };

                let feature_map_w = target_size / stride;
                let feature_map_h = target_size / stride;
                let anchors =
                    Self::generate_anchors(stride, &min_sizes[i], feature_map_w, feature_map_h);

                for (j, anchor) in anchors.iter().enumerate() {
                    let score = scores_data[j];
                    if score < confidence_threshold {
                        continue;
                    }

                    // Decode box: [l, t, r, b] (distances from center, normalized by stride)
                    // This assumes SCRFD model (det_10g.onnx) which outputs distances
                    let l = boxes_data[j * 4];
                    let t = boxes_data[j * 4 + 1];
                    let r = boxes_data[j * 4 + 2];
                    let b = boxes_data[j * 4 + 3];

                    // SCRFD uses stride-scaled distances
                    // x1 = cx - l * stride
                    // y1 = cy - t * stride
                    // x2 = cx + r * stride
                    // y2 = cy + b * stride

                    let x1 = anchor.cx - l * stride as f32;
                    let y1 = anchor.cy - t * stride as f32;
                    let x2 = anchor.cx + r * stride as f32;
                    let y2 = anchor.cy + b * stride as f32;

                    // Scale back to original image
                    // Use effective scale factors derived from actual resized dimensions
                    let inv_scale_x = original_width / new_w as f32;
                    let inv_scale_y = original_height / new_h as f32;

                    // Scale directly (no padding offset)
                    let original_x1 = x1 * inv_scale_x;
                    let original_y1 = y1 * inv_scale_y;
                    let original_x2 = x2 * inv_scale_x;
                    let original_y2 = y2 * inv_scale_y;

                    // 5 landmarks (eyes, nose, mouth corners) as stride-scaled offsets from the anchor
                    let landmarks = kps_data.and_then(|kps| {
                        let base = j * 10;
                        kps.get(base..base + 10).map(|p| {
                            (0..5)
                                .map(|k| {
                                    (
                                        (anchor.cx + p[k * 2] * stride as f32) * inv_scale_x,
                                        (anchor.cy + p[k * 2 + 1] * stride as f32) * inv_scale_y,
                                    )
                                })
                                .collect::<Vec<_>>()
                        })
                    });

                    all_detections.push(FaceBox {
                        x: original_x1,
                        y: original_y1,
                        width: original_x2 - original_x1,
                        height: original_y2 - original_y1,
                        confidence: score,
                        landmarks,
                    });
                }
            }

            all_detections
        };

        // Non-maximum suppression
        faces = self.nms(faces, 0.4);

        if faces.is_empty() {
            // No faces found after NMS
        }

        Ok(faces)
    }

    /// Generate anchors for a specific stride
    fn generate_anchors(
        stride: u32,
        min_sizes: &[u32],
        feature_w: u32,
        feature_h: u32,
    ) -> Vec<Anchor> {
        let mut anchors =
            Vec::with_capacity((feature_w * feature_h * min_sizes.len() as u32) as usize);

        for y in 0..feature_h {
            for x in 0..feature_w {
                for &_min_size in min_sizes {
                    // Dense anchor centers
                    // Adjusted to 0.0 (top-left) from 0.5 (center) to fix systematic bottom-right shift
                    let cx = (x as f32) * stride as f32;
                    let cy = (y as f32) * stride as f32;

                    anchors.push(Anchor { cx, cy });
                }
            }
        }
        anchors
    }

    /// Get face embedding implementation (from DynamicImage)
    fn get_face_embedding(
        &mut self,
        img: &DynamicImage,
        bbox: &FaceBox,
    ) -> Result<Vec<f32>, String> {
        // MobileFaceNet expects 112x112
        let target_size = 112;
        let aligned = bbox
            .landmarks
            .as_deref()
            .and_then(|lm| align_face(img, lm, target_size));
        let rgb_buf = match aligned {
            Some(face) => face,
            None => {
                // Fallback: padded bbox crop when landmarks are unavailable
                let padding = 0.2;
                let x = (bbox.x - bbox.width * padding).max(0.0) as u32;
                let y = (bbox.y - bbox.height * padding).max(0.0) as u32;
                let w = (bbox.width * (1.0 + 2.0 * padding)) as u32;
                let h = (bbox.height * (1.0 + 2.0 * padding)) as u32;
                let max_x = (x + w).min(img.width());
                let max_y = (y + h).min(img.height());
                img.crop_imm(x, y, max_x - x, max_y - y)
                    .resize_exact(
                        target_size,
                        target_size,
                        image::imageops::FilterType::Triangle,
                    )
                    .into_rgb8()
            }
        };
        // Embed the face and its mirror image and sum them (standard ArcFace
        // flip augmentation): steadier embeddings, especially for angled faces.
        let mut summed = self.run_embedding_model(&rgb_buf)?;
        let flipped = image::imageops::flip_horizontal(&rgb_buf);
        for (acc, v) in summed.iter_mut().zip(self.run_embedding_model(&flipped)?) {
            *acc += v;
        }

        // Normalize embedding to unit vector
        let norm: f32 = summed.iter().map(|x| x * x).sum::<f32>().sqrt();
        if !norm.is_finite() || norm <= f32::EPSILON {
            return Err("Invalid face embedding norm".to_string());
        }
        Ok(summed.iter().map(|x| x / norm).collect())
    }

    /// Run the embedding model on one 112x112 RGB face crop (raw, unnormalized output).
    fn run_embedding_model(&mut self, rgb_face: &image::RgbImage) -> Result<Vec<f32>, String> {
        // Normalize: (pixel - 127.5) / 128.0, planar RGB
        let mut array = Array::zeros((1, 3, 112, 112));
        for (x, y, pixel) in rgb_face.enumerate_pixels() {
            for c in 0..3 {
                array[[0, c, y as usize, x as usize]] = (pixel[c] as f32 - 127.5) / 128.0;
            }
        }

        let input_value = Value::from_array(array).map_err(|e| e.to_string())?;
        let outputs = self
            .embedding_model
            .as_mut()
            .unwrap()
            .run(inputs!["input.1" => input_value])
            .map_err(|e| format!("Embedding inference error: {}", e))?;
        let (_, embedding_data) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("Failed to extract embedding: {}", e))?;
        Ok(embedding_data.to_vec())
    }

    /// Compute cosine similarity between two embeddings
    #[allow(dead_code)]
    pub fn compare_faces(emb1: &[f32], emb2: &[f32]) -> f32 {
        if emb1.len() != emb2.len() {
            return 0.0;
        }
        // Embeddings are already normalized, so dot product = cosine similarity
        emb1.iter().zip(emb2.iter()).map(|(a, b)| a * b).sum()
    }

    /// Process image: detect all faces and get embeddings
    /// Filters out low-quality faces (low confidence, small size, blurry)
    pub fn process_image_from_bytes(
        &mut self,
        image_bytes: &[u8],
    ) -> Result<(Vec<FaceData>, (u32, u32)), String> {
        let img = image::load_from_memory(image_bytes)
            .map_err(|e| format!("Failed to load image from memory: {}", e))?;
        self.process_dynamic_image(&img)
    }

    fn process_dynamic_image(
        &mut self,
        img: &DynamicImage,
    ) -> Result<(Vec<FaceData>, (u32, u32)), String> {
        let faces = self.detect_faces(img)?;

        let mut results = Vec::new();
        for face in faces {
            // Filter 1: Skip low confidence faces
            if face.confidence < t_common::MIN_CONFIDENCE {
                continue;
            }

            // Filter 2: Skip very small faces (likely background people)
            // let face_area = face.width * face.height;
            // let img_width = img.width() as f32;
            // let img_height = img.height() as f32;
            // let img_area = img_width * img_height;
            // if face_area / img_area < t_common::MIN_FACE_RATIO {
            //     continue;
            // }

            // Filter 3: Skip faces smaller than minimum pixel size
            // if face.width < t_common::MIN_FACE_SIZE || face.height < t_common::MIN_FACE_SIZE {
            //     continue;
            // }

            // Filter 4: Skip blurry faces
            let blur_score = self.calculate_blur_score(img, &face);
            if blur_score < t_common::MIN_BLUR_SCORE {
                continue;
            }

            // Get embedding for quality face
            let embedding = self.get_face_embedding(img, &face)?;
            let low_quality = is_low_quality(&face) || blur_score < t_common::LOW_QUALITY_MIN_BLUR;
            results.push(FaceData {
                bbox: face,
                embedding,
                low_quality,
            });
        }

        Ok((results, (img.width(), img.height())))
    }

    /// Calculate blur score using Variance of Laplacian
    /// Optimized: Uses Welford's online algorithm to avoid allocating a large vector
    fn calculate_blur_score(&self, img: &DynamicImage, bbox: &FaceBox) -> f32 {
        let x = bbox.x.max(0.0) as u32;
        let y = bbox.y.max(0.0) as u32;
        // Check bounds to ensure we don't crash on cropping
        let w = bbox.width.min(img.width() as f32 - bbox.x) as u32;
        let h = bbox.height.min(img.height() as f32 - bbox.y) as u32;

        if w < 3 || h < 3 {
            return 0.0;
        }

        // Score at a fixed scale: per-pixel sharpness drops as resolution grows, so
        // large faces from full-size decodes would otherwise look blurry.
        const BLUR_SCORE_MAX_SIDE: u32 = 112;
        let mut crop = img.crop_imm(x, y, w, h);
        if w.max(h) > BLUR_SCORE_MAX_SIDE {
            crop = crop.resize(
                BLUR_SCORE_MAX_SIDE,
                BLUR_SCORE_MAX_SIDE,
                image::imageops::FilterType::Triangle,
            );
        }
        let crop = crop.to_luma8();
        let (width, height) = crop.dimensions();

        // Online variance calculation (Welford's algorithm)
        let mut count = 0usize;
        let mut m2 = 0.0;
        let mut mean = 0.0;

        for y in 1..height - 1 {
            for x in 1..width - 1 {
                let p = crop.get_pixel(x, y).0[0] as i16;
                let top = crop.get_pixel(x, y - 1).0[0] as i16;
                let bottom = crop.get_pixel(x, y + 1).0[0] as i16;
                let left = crop.get_pixel(x - 1, y).0[0] as i16;
                let right = crop.get_pixel(x + 1, y).0[0] as i16;

                let sum = top + bottom + left + right - 4 * p;
                let val = sum as f32;

                count += 1;
                let delta = val - mean;
                mean += delta / count as f32;
                let delta2 = val - mean;
                m2 += delta * delta2;
            }
        }

        if count < 2 {
            return 0.0;
        }

        // Variance
        m2 / (count as f32)
    }

    /// Non-maximum suppression
    fn nms(&self, mut boxes: Vec<FaceBox>, iou_threshold: f32) -> Vec<FaceBox> {
        boxes.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));

        let mut keep = Vec::new();
        let mut suppressed = vec![false; boxes.len()];

        for i in 0..boxes.len() {
            if suppressed[i] {
                continue;
            }
            keep.push(boxes[i].clone());

            for j in (i + 1)..boxes.len() {
                if suppressed[j] {
                    continue;
                }
                if self.iou(&boxes[i], &boxes[j]) > iou_threshold {
                    suppressed[j] = true;
                }
            }
        }

        keep
    }

    /// Intersection over Union
    /// Optimized: Simplified redundant max(0.0) for valid boxes
    fn iou(&self, a: &FaceBox, b: &FaceBox) -> f32 {
        let x1 = a.x.max(b.x);
        let y1 = a.y.max(b.y);
        let x2 = (a.x + a.width).min(b.x + b.width);
        let y2 = (a.y + a.height).min(b.y + b.height);

        if x2 <= x1 || y2 <= y1 {
            return 0.0;
        }

        let inter_area = (x2 - x1) * (y2 - y1);
        let a_area = a.width * a.height;
        let b_area = b.width * b.height;

        inter_area / (a_area + b_area - inter_area)
    }
}

#[derive(Clone)]
pub struct FaceState(pub std::sync::Arc<Mutex<FaceEngine>>);

/// A rescan detection overlapping a stored face this much is the same face.
const RESCAN_SAME_FACE_IOU: f32 = 0.3;

/// Intersection over union of two (x, y, width, height) boxes.
fn box_iou(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> f32 {
    let ix = (a.0 + a.2).min(b.0 + b.2) - a.0.max(b.0);
    let iy = (a.1 + a.3).min(b.1 + b.3) - a.1.max(b.1);
    if ix <= 0.0 || iy <= 0.0 {
        return 0.0;
    }
    let inter = ix * iy;
    let union = a.2 * a.3 + b.2 * b.3 - inter;
    if union <= 0.0 { 0.0 } else { inter / union }
}

/// Canonical ArcFace landmark positions in a 112x112 crop
/// (left eye, right eye, nose tip, left mouth corner, right mouth corner).
const ARCFACE_TEMPLATE: [(f32, f32); 5] = [
    (38.2946, 51.6963),
    (73.5318, 51.5014),
    (56.0252, 71.7366),
    (41.5493, 92.3655),
    (70.7299, 92.2041),
];

/// Least-squares similarity transform (rotation + uniform scale + translation)
/// mapping `src` points onto `dst`, as `[a, b, tx, ty]` where
/// `x' = a*x - b*y + tx` and `y' = b*x + a*y + ty`.
fn estimate_similarity(src: &[(f32, f32)], dst: &[(f32, f32)]) -> Option<[f32; 4]> {
    let n = src.len().min(dst.len());
    if n < 2 {
        return None;
    }
    let nf = n as f32;
    let (smx, smy) = src[..n]
        .iter()
        .fold((0.0, 0.0), |acc, p| (acc.0 + p.0 / nf, acc.1 + p.1 / nf));
    let (dmx, dmy) = dst[..n]
        .iter()
        .fold((0.0, 0.0), |acc, p| (acc.0 + p.0 / nf, acc.1 + p.1 / nf));

    let (mut num_a, mut num_b, mut den) = (0.0f32, 0.0f32, 0.0f32);
    for k in 0..n {
        let (sx, sy) = (src[k].0 - smx, src[k].1 - smy);
        let (dx, dy) = (dst[k].0 - dmx, dst[k].1 - dmy);
        num_a += sx * dx + sy * dy;
        num_b += sx * dy - sy * dx;
        den += sx * sx + sy * sy;
    }
    if den <= f32::EPSILON {
        return None;
    }
    let a = num_a / den;
    let b = num_b / den;
    let tx = dmx - (a * smx - b * smy);
    let ty = dmy - (b * smx + a * smy);
    Some([a, b, tx, ty])
}

/// Warp the face into the canonical ArcFace pose using its 5 landmarks,
/// with bilinear sampling and black outside the source image.
fn align_face(
    img: &DynamicImage,
    landmarks: &[(f32, f32)],
    size: u32,
) -> Option<image::RgbImage> {
    use image::GenericImageView;

    if landmarks.len() != 5 {
        return None;
    }
    let scale = size as f32 / 112.0;
    let template: Vec<(f32, f32)> = ARCFACE_TEMPLATE
        .iter()
        .map(|&(x, y)| (x * scale, y * scale))
        .collect();
    let [a, b, tx, ty] = estimate_similarity(landmarks, &template)?;
    let det = a * a + b * b;
    if det <= f32::EPSILON || !det.is_finite() {
        return None;
    }

    let (w, h) = img.dimensions();
    let sample = |x: i64, y: i64| -> [f32; 3] {
        if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
            return [0.0; 3];
        }
        let p = img.get_pixel(x as u32, y as u32);
        [p[0] as f32, p[1] as f32, p[2] as f32]
    };

    let mut out = image::RgbImage::new(size, size);
    for (u, v, pixel) in out.enumerate_pixels_mut() {
        // Inverse similarity: src = R^-1 * (dst - t)
        let du = u as f32 - tx;
        let dv = v as f32 - ty;
        let sx = (a * du + b * dv) / det;
        let sy = (-b * du + a * dv) / det;

        let x0 = sx.floor();
        let y0 = sy.floor();
        let fx = sx - x0;
        let fy = sy - y0;
        let (x0, y0) = (x0 as i64, y0 as i64);

        let p00 = sample(x0, y0);
        let p10 = sample(x0 + 1, y0);
        let p01 = sample(x0, y0 + 1);
        let p11 = sample(x0 + 1, y0 + 1);
        for c in 0..3 {
            let top = p00[c] * (1.0 - fx) + p10[c] * fx;
            let bottom = p01[c] * (1.0 - fx) + p11[c] * fx;
            pixel[c] = (top * (1.0 - fy) + bottom * fy).round().clamp(0.0, 255.0) as u8;
        }
    }
    Some(out)
}

pub fn run_face_indexing(
    app_handle: AppHandle,
    face_state: FaceState,
    cancel_token_struct: FaceIndexCancellation,
    status_token_struct: FaceIndexingStatus,
    progress_token_struct: FaceIndexProgressState,
    cluster_epsilon: Option<f32>,
    rescan: bool,
) -> Result<(), String> {
    let cancel_token = cancel_token_struct.0.clone();
    let status_token = status_token_struct.0.clone();
    let progress_token = progress_token_struct.0.clone();
    // Use provided epsilon or default to 0.42
    let epsilon = cluster_epsilon.unwrap_or(0.42);

    // Check if already running
    {
        let mut running = status_token.lock().unwrap();
        if *running {
            return Err("Face indexing is already running".to_string());
        }
        *running = true;
    }

    // Reset cancellation flag
    *cancel_token.lock().unwrap() = false;

    // Reset progress
    {
        let mut progress = progress_token.lock().unwrap();
        progress.current = 0;
        progress.total = 0;
        progress.faces_found = 0;
        progress.phase = "indexing".to_string();
    }

    tauri::async_runtime::spawn(async move {
        // 1. Initialization
        let reset_status = || {
            if let Ok(mut running) = status_token.lock() {
                *running = false;
            }
        };

        // Load models if not already loaded
        {
            let mut engine = face_state.0.lock().unwrap();
            if !engine.is_loaded() {
                if let Err(e) = engine.load_models(&app_handle) {
                    eprintln!("Failed to load face models: {}", e);
                    let _ = app_handle.emit(
                        "face_index_finished",
                        serde_json::json!({
                            "total_faces": 0,
                            "total_persons": 0,
                            "cancelled": false,
                            "error": e.to_string()
                        }),
                    );
                    reset_status();
                    return;
                }
            }
        }

        // 2. Preparation (Get files and stats)
        let (processed_count, existing_faces_count) = match t_sqlite::Face::get_stats() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Failed to get stats: {}", e);
                (0, 0)
            }
        };
        // A rescan revisits every image (keeping stored faces), so progress starts at zero.
        let processed_count = if rescan { 0 } else { processed_count };

        let files = match t_sqlite::Face::get_image_files_for_faces(rescan) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Failed to get unprocessed files: {}", e);
                let _ = app_handle.emit(
                    "face_index_finished",
                    serde_json::json!({
                        "total_faces": 0,
                        "total_persons": 0,
                        "cancelled": false,
                        "error": e
                    }),
                );
                reset_status();
                return;
            }
        };

        let total_files = processed_count + files.len();
        let mut total_faces = existing_faces_count;
        let mut current = processed_count;

        // Init progress
        {
            let mut progress = progress_token.lock().unwrap();
            progress.total = total_files;
            progress.current = current;
            progress.faces_found = total_faces;
            progress.phase = "indexing".to_string();
        }

        let _ = app_handle.emit(
            "face_index_progress",
            serde_json::json!({
                "current": current,
                "total": total_files,
                "faces_found": total_faces,
                "phase": "indexing"
            }),
        );

        // 3. Image Processing Loop
        let mut cancelled = false;
        let db_conn = match t_sqlite::open_conn() {
            Ok(conn) => conn,
            Err(e) => {
                eprintln!("Failed to open DB connection for face indexing: {}", e);
                let _ = app_handle.emit(
                    "face_index_finished",
                    serde_json::json!({
                        "total_faces": 0,
                        "total_persons": 0,
                        "cancelled": false,
                        "error": e
                    }),
                );
                reset_status();
                return;
            }
        };

        for (file_id, file_path, width, height) in files {
            if *cancel_token.lock().unwrap() {
                cancelled = true;
                break;
            }

            current += 1;

            // Prefer a large, orientation-corrected decode of the local original: small
            // stored thumbnails leave background faces only a few pixels wide for the
            // embedding model. Fall back to the stored thumbnail (e.g. cloud-only files).
            let original_local = !crate::t_utils::is_cloud_only_path(&file_path)
                && crate::t_utils::content_readable(&file_path);
            let decoded = if original_local {
                crate::t_image::decode_upright_for_faces(&file_path, t_common::FACE_DECODE_SIZE)
            } else {
                None
            };
            let thumb_bytes = match decoded {
                Some(_) => None,
                None => t_sqlite::AThumb::fetch(file_id)
                    .ok()
                    .flatten()
                    .and_then(|thumb| thumb.thumb_data),
            };
            let Some(image_bytes) = decoded.or(thumb_bytes) else {
                // Reading the online-only original would download it; retry once it is local.
                if !original_local {
                    continue;
                }
                eprintln!("Failed to decode image for faces: {}", file_path);
                if rescan {
                    continue; // keep the earlier result for this file
                }
                if let Err(e) = t_sqlite::Face::mark_scanned_with_conn(&db_conn, file_id, 2) {
                    eprintln!("Failed to mark unreadable file {} as skipped: {}", file_id, e);
                }
                continue;
            };
            let mut engine = face_state.0.lock().unwrap();
            let process_result = engine.process_image_from_bytes(&image_bytes);
            drop(engine);

            match process_result {
                Ok((mut faces, (proc_w, proc_h))) => {
                    // Scale bbox from the decoded image to original size
                    {
                        let scale_x = width as f32 / proc_w as f32;
                        let scale_y = height as f32 / proc_h as f32;

                        for face in &mut faces {
                            face.bbox.x *= scale_x;
                            face.bbox.y *= scale_y;
                            face.bbox.width *= scale_x;
                            face.bbox.height *= scale_y;
                        }
                    }

                    // Rescan: keep stored faces (and their names); add only detections
                    // that don't overlap a stored face.
                    let mut already_had_faces = false;
                    if rescan {
                        let existing = t_sqlite::Face::bboxes_for_file_with_conn(&db_conn, file_id)
                            .unwrap_or_default();
                        already_had_faces = !existing.is_empty();
                        faces.retain(|face| {
                            let b = &face.bbox;
                            !existing.iter().any(|&(x, y, w, h)| {
                                box_iou((b.x, b.y, b.width, b.height), (x, y, w, h))
                                    >= RESCAN_SAME_FACE_IOU
                            })
                        });
                    }

                    let has_faces = !faces.is_empty();
                    let status = if has_faces || already_had_faces { 1 } else { 2 };

                    if let Err(e) =
                        t_sqlite::Face::mark_scanned_with_conn(&db_conn, file_id, status)
                    {
                        eprintln!("Failed to mark file {} as scanned: {}", file_id, e);
                    }

                    if has_faces {
                        for face_data in &faces {
                            let bbox_json = serde_json::json!({
                                "x": face_data.bbox.x,
                                "y": face_data.bbox.y,
                                "width": face_data.bbox.width,
                                "height": face_data.bbox.height,
                                "confidence": face_data.bbox.confidence,
                            })
                            .to_string();

                            match t_sqlite::Face::add_with_conn(
                                &db_conn,
                                file_id,
                                &bbox_json,
                                &face_data.embedding,
                                face_data.low_quality,
                            ) {
                                Ok(_) => total_faces += 1,
                                Err(e) => eprintln!("Failed to store face: {}", e),
                            }
                        }
                    }
                }
                Err(e) => {
                    if !crate::t_utils::content_readable(&file_path) { continue; }
                    eprintln!("Failed to process image {}: {}", file_path, e);
                    if rescan {
                        continue; // keep the earlier result for this file
                    }
                    // Do not retry files the decoder cannot read on every resume.
                    // Status 2 means the image was processed without a face result.
                    if let Err(mark_error) = t_sqlite::Face::mark_scanned_with_conn(&db_conn, file_id, 2) {
                        eprintln!("Failed to mark unreadable file {} as skipped: {}", file_id, mark_error);
                    }
                }
            }

            // Periodic progress update (every 10 files or at end)
            if current % 10 == 0 || current == total_files {
                {
                    let mut progress = progress_token.lock().unwrap();
                    progress.current = current;
                    progress.faces_found = total_faces;
                }

                let _ = app_handle.emit(
                    "face_index_progress",
                    serde_json::json!({
                        "current": current,
                        "total": total_files,
                        "faces_found": total_faces,
                        "phase": "indexing"
                    }),
                );
            }
        }

        if cancelled {
            let _ = app_handle.emit(
                "face_index_finished",
                serde_json::json!({
                    "total_faces": total_faces,
                    "total_persons": 0,
                    "cancelled": true
                }),
            );
            reset_status();
            return;
        }

        // 4. Clustering
        {
            let mut progress = progress_token.lock().unwrap();
            progress.phase = "clustering".to_string();
        }

        let _ = app_handle.emit(
            "face_index_progress",
            serde_json::json!({
                "current": total_files,
                "total": total_files,
                "faces_found": total_faces,
                "phase": "clustering"
            }),
        );

        let cancel_token_cluster = cancel_token.clone();
        let total_persons = match t_cluster::cluster_faces(
            epsilon,
            |progress| {
                let _ = app_handle.emit(
                    "cluster_progress",
                    serde_json::json!({
                        "phase": progress.phase,
                        "current": progress.current,
                        "total": progress.total,
                    }),
                );
            },
            || {
                // Check if user has cancelled
                *cancel_token_cluster.lock().unwrap()
            },
        ) {
            Ok(count) => count,
            Err(e) => {
                eprintln!("Clustering failed: {}", e);
                0
            }
        };
        let cancelled_during_cluster = *cancel_token.lock().unwrap();

        // 5. Finished
        let _ = app_handle.emit(
            "face_index_finished",
            serde_json::json!({
                "total_faces": total_faces,
                "total_persons": total_persons,
                "cancelled": cancelled_during_cluster
            }),
        );
        reset_status();
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn face_with(width: f32, confidence: f32, nose_x: f32) -> FaceBox {
        // Eyes 40px apart at y=50; nose_x is relative to the eye midpoint (x=50).
        FaceBox {
            x: 0.0,
            y: 0.0,
            width,
            height: width,
            confidence,
            landmarks: Some(vec![(30.0, 50.0), (70.0, 50.0), (50.0 + nose_x, 70.0), (35.0, 90.0), (65.0, 90.0)]),
        }
    }

    #[test]
    fn low_quality_flags_small_turned_and_uncertain_faces() {
        assert!(!is_low_quality(&face_with(120.0, 0.9, 0.0)));
        assert!(!is_low_quality(&face_with(120.0, 0.9, 12.0))); // mildly turned
        assert!(is_low_quality(&face_with(30.0, 0.9, 0.0))); // small
        assert!(is_low_quality(&face_with(120.0, 0.7, 0.0))); // uncertain
        assert!(is_low_quality(&face_with(120.0, 0.9, 22.0))); // strongly turned
        assert!(is_low_quality(&face_with(120.0, 0.9, -22.0)));
    }

    #[test]
    fn box_iou_detects_the_same_face() {
        assert!((box_iou((0.0, 0.0, 10.0, 10.0), (0.0, 0.0, 10.0, 10.0)) - 1.0).abs() < 1e-6);
        assert!(box_iou((0.0, 0.0, 10.0, 10.0), (1.0, 1.0, 10.0, 10.0)) >= RESCAN_SAME_FACE_IOU);
        assert_eq!(box_iou((0.0, 0.0, 10.0, 10.0), (20.0, 0.0, 10.0, 10.0)), 0.0);
    }

    #[test]
    fn similarity_recovers_known_transform() {
        // rotate 30deg, scale 1.7, translate (12, -4)
        let (s, th) = (1.7f32, 30f32.to_radians());
        let (a, b) = (s * th.cos(), s * th.sin());
        let src = [(10.0, 20.0), (40.0, 18.0), (25.0, 35.0), (12.0, 50.0), (38.0, 52.0)];
        let dst: Vec<_> = src
            .iter()
            .map(|&(x, y)| (a * x - b * y + 12.0, b * x + a * y - 4.0))
            .collect();
        let [ea, eb, etx, ety] = estimate_similarity(&src, &dst).unwrap();
        assert!((ea - a).abs() < 1e-4 && (eb - b).abs() < 1e-4);
        assert!((etx - 12.0).abs() < 1e-3 && (ety + 4.0).abs() < 1e-3);
    }

    #[test]
    fn align_face_maps_landmarks_onto_template() {
        // Place bright dots at landmark positions of a scaled/shifted face and
        // check they land on the ArcFace template after alignment.
        let mut img = image::RgbImage::new(400, 400);
        let lm: Vec<(f32, f32)> = ARCFACE_TEMPLATE
            .iter()
            .map(|&(x, y)| (x * 2.0 + 100.0, y * 2.0 + 60.0))
            .collect();
        for &(x, y) in &lm {
            for dy in -2i32..=2 {
                for dx in -2i32..=2 {
                    img.put_pixel((x as i32 + dx) as u32, (y as i32 + dy) as u32, image::Rgb([255, 255, 255]));
                }
            }
        }
        let out = align_face(&DynamicImage::ImageRgb8(img), &lm, 112).unwrap();
        for &(x, y) in &ARCFACE_TEMPLATE {
            assert!(out.get_pixel(x.round() as u32, y.round() as u32)[0] > 200);
        }
    }

    /// Runs the bundled models on demo images and writes aligned crops to
    /// $FACE_ALIGN_OUT for visual inspection (input dir: $FACE_ALIGN_IN, default demo thumbnails).
    /// `FACE_ALIGN_OUT=/some/dir cargo test --lib aligned_crops -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn aligned_crops_from_real_models() {
        let models = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/models");
        let load = |name: &str| {
            Session::builder()
                .unwrap()
                .commit_from_file(models.join(name))
                .unwrap()
        };
        let mut engine = FaceEngine {
            detection_model: Some(load(t_common::DETECTION_MODEL)),
            embedding_model: Some(load(t_common::EMBEDDING_MODEL)),
        };
        let out_dir = std::path::PathBuf::from(std::env::var("FACE_ALIGN_OUT").unwrap());
        let demo = std::env::var("FACE_ALIGN_IN").map(std::path::PathBuf::from).unwrap_or_else(|_| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/public/demos/lap/thumbnails")
        });
        let mut found = 0;
        for entry in std::fs::read_dir(demo).unwrap() {
            let path = entry.unwrap().path();
            let img = image::open(&path).unwrap();
            for (i, face) in engine.detect_faces(&img).unwrap().iter().enumerate() {
                println!(
                    "{} face {} at x={:.0} y={:.0} w={:.0} confidence {:.4} blur {:.1} low_quality {}",
                    path.display(),
                    i,
                    face.x,
                    face.y,
                    face.width,
                    face.confidence,
                    engine.calculate_blur_score(&img, face),
                    is_low_quality(face)
                );
                let lm = face.landmarks.as_ref().expect("detector returned no landmarks");
                let crop = align_face(&img, lm, 112).unwrap();
                let name = format!("{}_{}.png", path.file_stem().unwrap().to_string_lossy(), i);
                crop.save(out_dir.join(name)).unwrap();
                engine.get_face_embedding(&img, face).unwrap();
                found += 1;
            }
        }
        println!("aligned {} faces", found);
        assert!(found > 0);
    }

    /// Rotated-photo check: stores $FACE_ALIGN_IN/crew.jpg sideways with EXIF
    /// orientation 6 (like a phone portrait), indexes it, and writes the person-avatar
    /// crops to $FACE_ALIGN_OUT for inspection.
    #[test]
    #[ignore]
    fn face_crops_match_detected_faces_on_rotated_photos() {
        use rusqlite::{Connection, params};
        let input = std::path::PathBuf::from(std::env::var("FACE_ALIGN_IN").unwrap()).join("crew.jpg");
        let out_dir = std::path::PathBuf::from(std::env::var("FACE_ALIGN_OUT").unwrap());
        let rotated = out_dir.join("rotated6.jpg");
        image::open(&input).unwrap().rotate270().save(&rotated).unwrap();
        let mut metadata = little_exif::metadata::Metadata::new();
        metadata.set_tag(little_exif::exif_tag::ExifTag::Orientation(vec![6]));
        metadata.write_to_file(&rotated).unwrap();
        let path = rotated.to_string_lossy().to_string();
        assert_eq!(crate::t_image::get_image_orientation(&path), 6);

        let models = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/models");
        let load = |name: &str| Session::builder().unwrap().commit_from_file(models.join(name)).unwrap();
        let mut engine = FaceEngine {
            detection_model: Some(load(t_common::DETECTION_MODEL)),
            embedding_model: Some(load(t_common::EMBEDDING_MODEL)),
        };
        // Same decode as indexing; stored dimensions are upright (swapped for orientation 6).
        let bytes = crate::t_image::get_image_thumbnail_with_encoding(&path, 6, t_common::FACE_DECODE_SIZE, true)
            .unwrap()
            .unwrap();
        let (faces, (proc_w, proc_h)) = engine.process_image_from_bytes(&bytes).unwrap();
        let (raw_w, raw_h) = image::image_dimensions(&rotated).unwrap();
        let (stored_w, stored_h) = (raw_h, raw_w);
        assert_eq!((proc_w > proc_h), (stored_w > stored_h), "decode must be upright");
        assert_eq!(faces.len(), 3);

        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE afolders(id INTEGER PRIMARY KEY, path TEXT);
             CREATE TABLE afiles(id INTEGER PRIMARY KEY, folder_id INTEGER, name TEXT, width INTEGER, height INTEGER, e_orientation INTEGER);
             CREATE TABLE faces(id INTEGER PRIMARY KEY, file_id INTEGER, bbox TEXT, person_id INTEGER);",
        )
        .unwrap();
        conn.execute("INSERT INTO afolders VALUES (1, ?1)", params![out_dir.to_string_lossy()]).unwrap();
        conn.execute(
            "INSERT INTO afiles VALUES (1, 1, 'rotated6.jpg', ?1, ?2, 6)",
            params![stored_w, stored_h],
        )
        .unwrap();
        let (sx, sy) = (stored_w as f32 / proc_w as f32, stored_h as f32 / proc_h as f32);
        for (i, face) in faces.iter().enumerate() {
            let bbox = serde_json::json!({
                "x": face.bbox.x * sx, "y": face.bbox.y * sy,
                "width": face.bbox.width * sx, "height": face.bbox.height * sy,
            });
            conn.execute("INSERT INTO faces VALUES (?1, 1, ?2, NULL)", params![i as i64 + 1, bbox.to_string()]).unwrap();
            let crop = t_sqlite::Person::generate_face_thumbnail(&conn, i as i64 + 1).unwrap().unwrap();
            std::fs::write(out_dir.join(format!("avatar_{}.jpg", i)), crop).unwrap();
        }
    }
}
