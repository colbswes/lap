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
    pet_model: Option<Session>,       // DINOv2 (optional)
}

/// A face or pet found in an image, ready to store.
#[derive(Debug, Clone)]
pub struct Detection {
    pub bbox: FaceBox,
    pub embedding: Vec<f32>,
    pub low_quality: bool,
    /// "face" or "pet" (`faces.kind`).
    pub kind: &'static str,
    pub species: Option<&'static str>,
}

impl FaceEngine {
    /// Engine with the bundled face models (no pet model), for tests that run real inference.
    #[cfg(test)]
    pub(crate) fn load_bundled_for_test() -> Self {
        let models = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/models");
        let load = |name: &str| Session::builder().unwrap().commit_from_file(models.join(name)).unwrap();
        Self {
            detection_model: Some(load(t_common::DETECTION_MODEL)),
            embedding_model: Some(load(t_common::EMBEDDING_MODEL)),
            pet_model: None,
        }
    }

    pub fn new() -> Self {
        Self {
            detection_model: None,
            embedding_model: None,
            pet_model: None,
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

        // Pet model is optional: without it, indexing finds faces only.
        let pet_model_path = resource_dir.join(t_common::PET_EMBEDDING_MODEL);
        if pet_model_path.exists() {
            match Session::builder()
                .and_then(|b| b.with_optimization_level(GraphOptimizationLevel::Level3))
                .and_then(|b| b.with_intra_threads(4))
                .and_then(|b| b.commit_from_file(&pet_model_path))
            {
                Ok(session) => self.pet_model = Some(session),
                Err(e) => eprintln!("Failed to load pet model: {}", e),
            }
        }

        Ok(())
    }

    pub fn is_loaded(&self) -> bool {
        self.detection_model.is_some() && self.embedding_model.is_some()
    }

    /// Detect faces implementation (from DynamicImage)
    fn detect_faces(&mut self, img: &DynamicImage) -> Result<Vec<FaceBox>, String> {
        self.detect_faces_with_threshold(img, t_common::MIN_CONFIDENCE)
    }

    fn detect_faces_with_threshold(
        &mut self,
        img: &DynamicImage,
        confidence_threshold: f32,
    ) -> Result<Vec<FaceBox>, String> {
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
    /// Faces and pets in an encoded image, with boxes in decoded-image pixels.
    pub fn detect_all_from_bytes(
        &mut self,
        image_bytes: &[u8],
    ) -> Result<(Vec<Detection>, (u32, u32)), String> {
        let img = image::load_from_memory(image_bytes)
            .map_err(|e| format!("Failed to load image from memory: {}", e))?;
        let (faces, dims) = self.process_dynamic_image(&img)?;
        let pets = self.detect_pets(image_bytes, &img);
        let mut detections: Vec<Detection> = faces
            .into_iter()
            .filter(|f| !is_human_false_positive_in_pet(&f.bbox, &pets))
            .map(|f| Detection { bbox: f.bbox, embedding: f.embedding, low_quality: f.low_quality, kind: "face", species: None })
            .collect();
        detections.extend(pets);
        Ok((detections, dims))
    }

    /// Detect only pets in an encoded image, with boxes in decoded-image pixels.
    pub fn detect_pets_from_bytes(
        &mut self,
        image_bytes: &[u8],
    ) -> Result<(Vec<Detection>, (u32, u32)), String> {
        let img = image::load_from_memory(image_bytes)
            .map_err(|e| format!("Failed to load image from memory: {}", e))?;
        let dims = (img.width(), img.height());
        let pets = self.detect_pets(image_bytes, &img);
        Ok((pets, dims))
    }

    /// Embed a normalized (top-left origin) crop `(x, y, width, height)` using the pet model.
    pub fn embed_pet_box_from_bytes(
        &mut self,
        image_bytes: &[u8],
        norm_bbox: (f32, f32, f32, f32),
    ) -> Result<Vec<f32>, String> {
        if self.pet_model.is_none() {
            return Err("Pet model not loaded".to_string());
        }
        let img = image::load_from_memory(image_bytes)
            .map_err(|e| format!("Failed to load image from memory: {}", e))?;
        let (w, h) = (img.width() as f32, img.height() as f32);
        let pixel_box = (
            norm_bbox.0 * w,
            norm_bbox.1 * h,
            norm_bbox.2 * w,
            norm_bbox.3 * h,
        );
        let crop = crate::t_pet::square_crop(&img, pixel_box, 224);
        self.run_pet_model(&crop)
    }

    /// Try to produce an aligned human-face embedding for a manually drawn box
    /// (normalized top-left `(x, y, width, height)`). Runs RetinaFace on a padded crop
    /// at a gentler threshold to locate 5-point landmarks; returns `Ok(None)` if no
    /// landmarked face matches the box, so unaligned crops never pollute clustering.
    pub fn embed_face_box_from_bytes(
        &mut self,
        image_bytes: &[u8],
        norm_bbox: (f32, f32, f32, f32),
    ) -> Result<Option<Vec<f32>>, String> {
        if self.detection_model.is_none() || self.embedding_model.is_none() {
            return Ok(None);
        }
        let img = image::load_from_memory(image_bytes)
            .map_err(|e| format!("Failed to load image from memory: {}", e))?;
        let (img_w, img_h) = (img.width() as f32, img.height() as f32);
        let bx = (norm_bbox.0 * img_w).clamp(0.0, img_w);
        let by = (norm_bbox.1 * img_h).clamp(0.0, img_h);
        let bw = (norm_bbox.2 * img_w).clamp(1.0, img_w - bx);
        let bh = (norm_bbox.3 * img_h).clamp(1.0, img_h - by);

        let pad_x = bw * 0.5;
        let pad_y = bh * 0.5;
        let cx0 = (bx - pad_x).max(0.0).floor() as u32;
        let cy0 = (by - pad_y).max(0.0).floor() as u32;
        let cx1 = (bx + bw + pad_x).min(img_w).ceil() as u32;
        let cy1 = (by + bh + pad_y).min(img_h).ceil() as u32;
        if cx1 <= cx0 + 4 || cy1 <= cy0 + 4 {
            return Ok(None);
        }

        let crop = img.crop_imm(cx0, cy0, cx1 - cx0, cy1 - cy0);
        let detected = self.detect_faces_with_threshold(&crop, 0.25)?;
        let target = (bx, by, bw, bh);
        let best = detected
            .into_iter()
            .filter(|f| f.landmarks.as_ref().is_some_and(|lm| lm.len() == 5))
            .map(|mut f| {
                f.x += cx0 as f32;
                f.y += cy0 as f32;
                if let Some(ref mut lm) = f.landmarks {
                    for pt in lm.iter_mut() {
                        pt.0 += cx0 as f32;
                        pt.1 += cy0 as f32;
                    }
                }
                let iou = box_iou((f.x, f.y, f.width, f.height), target);
                (iou, f)
            })
            .filter(|(iou, _)| *iou >= 0.25)
            .max_by(|a, b| a.0.total_cmp(&b.0));

        let Some((_, matched)) = best else {
            return Ok(None);
        };
        Ok(self.get_face_embedding(&img, &matched).ok())
    }

    /// Cats and dogs via Apple Vision, embedded with the pet model.
    fn detect_pets(&mut self, image_bytes: &[u8], img: &DynamicImage) -> Vec<Detection> {
        if self.pet_model.is_none() {
            return Vec::new();
        }
        let animals = match crate::t_pet::detect_animals(image_bytes) {
            Ok(animals) => animals,
            Err(e) => {
                eprintln!("Animal detection failed: {}", e);
                return Vec::new();
            }
        };
        let (w, h) = (img.width() as f32, img.height() as f32);
        let mut out = Vec::new();
        for animal in animals {
            let bbox = FaceBox {
                x: animal.x * w,
                y: animal.y * h,
                width: animal.width * w,
                height: animal.height * h,
                confidence: animal.confidence,
                landmarks: None,
            };
            let crop = crate::t_pet::square_crop(img, (bbox.x, bbox.y, bbox.width, bbox.height), 224);
            match self.run_pet_model(&crop) {
                Ok(embedding) => out.push(Detection {
                    low_quality: bbox.width.min(bbox.height) < t_common::PET_LOW_QUALITY_MIN_PX
                        || animal.confidence < t_common::PET_LOW_QUALITY_MIN_CONFIDENCE,
                    bbox,
                    embedding,
                    kind: "pet",
                    species: Some(animal.kind.as_str()),
                }),
                Err(e) => eprintln!("Pet embedding failed: {}", e),
            }
        }
        out
    }

    /// DINOv2 embedding of a 224x224 crop and its mirror image (averaged, unit length).
    fn run_pet_model(&mut self, crop: &image::RgbImage) -> Result<Vec<f32>, String> {
        let flipped = image::imageops::flip_horizontal(crop);
        let mut batch = Array::zeros((2, 3, 224, 224));
        for (b, img) in [crop, &flipped].into_iter().enumerate() {
            for (x, y, pixel) in img.enumerate_pixels() {
                for c in 0..3 {
                    batch[[b, c, y as usize, x as usize]] =
                        (pixel[c] as f32 / 255.0 - crate::t_pet::IMAGENET_MEAN[c]) / crate::t_pet::IMAGENET_STD[c];
                }
            }
        }
        let input = Value::from_array(batch).map_err(|e| e.to_string())?;
        let outputs = self
            .pet_model
            .as_mut()
            .ok_or("Pet model not loaded")?
            .run(inputs!["pixel_values" => input])
            .map_err(|e| format!("Pet embedding inference error: {}", e))?;
        let (_, data) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("Failed to extract pet embedding: {}", e))?;
        let dim = data.len() / 2;
        let summed: Vec<f32> = (0..dim).map(|i| data[i] + data[dim + i]).collect();
        let norm = summed.iter().map(|v| v * v).sum::<f32>().sqrt();
        if !norm.is_finite() || norm <= f32::EPSILON {
            return Err("Invalid pet embedding norm".to_string());
        }
        Ok(summed.iter().map(|v| v / norm).collect())
    }

    /// Faces only (no pet detection), with boxes in decoded-image pixels.
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

            // Filter 4: Skip blurry low-confidence detections. High-confidence faces
            // (>= LOW_QUALITY_MIN_CONFIDENCE) are kept even if soft/dim so obvious
            // faces aren't missed; if soft they are flagged `low_quality` so they
            // never bridge clusters.
            let blur_score = self.calculate_blur_score(img, &face);
            if blur_score < t_common::MIN_BLUR_SCORE
                && face.confidence < t_common::LOW_QUALITY_MIN_CONFIDENCE
            {
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

    /// Calculate blur score using contrast-compensated Variance of Laplacian.
    /// Raw Laplacian variance scales quadratically with image brightness/contrast, which
    /// unfairly penalizes sharp faces in dim lighting. We gently normalize low-contrast
    /// crops (up to 4x gain, with a noise floor) so dark scenes keep real faces.
    /// Optimized: Uses Welford's online algorithm to avoid allocating a large vector.
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

        // Online variance calculation (Welford's algorithm) for Laplacian and luma
        let mut count = 0usize;
        let mut lap_m2 = 0.0f32;
        let mut lap_mean = 0.0f32;
        let mut luma_m2 = 0.0f32;
        let mut luma_mean = 0.0f32;

        for y in 1..height - 1 {
            for x in 1..width - 1 {
                let p = crop.get_pixel(x, y).0[0] as i16;
                let top = crop.get_pixel(x, y - 1).0[0] as i16;
                let bottom = crop.get_pixel(x, y + 1).0[0] as i16;
                let left = crop.get_pixel(x - 1, y).0[0] as i16;
                let right = crop.get_pixel(x + 1, y).0[0] as i16;

                let sum = top + bottom + left + right - 4 * p;
                let val = sum as f32;
                let luma = p as f32;

                count += 1;
                let c = count as f32;
                let delta = val - lap_mean;
                lap_mean += delta / c;
                let delta2 = val - lap_mean;
                lap_m2 += delta * delta2;

                let l_delta = luma - luma_mean;
                luma_mean += l_delta / c;
                let l_delta2 = luma - luma_mean;
                luma_m2 += l_delta * l_delta2;
            }
        }

        if count < 2 {
            return 0.0;
        }

        let lap_var = lap_m2 / (count as f32);
        let luma_std = (luma_m2 / (count as f32)).sqrt();
        // Gently boost dim/low-contrast crops (std < 45) up to 4x, while tapering the
        // boost back to 1x on nearly flat patches (std < 6) so sensor noise isn't amplified.
        const TARGET_LUMA_STD: f32 = 45.0;
        const FLAT_LUMA_STD: f32 = 6.0;
        const MAX_CONTRAST_BOOST: f32 = 4.0;
        let raw_boost = (TARGET_LUMA_STD / luma_std.max(FLAT_LUMA_STD)).clamp(1.0, MAX_CONTRAST_BOOST);
        let ramp = (luma_std / FLAT_LUMA_STD).clamp(0.0, 1.0);
        let boost = 1.0 + (raw_boost - 1.0) * ramp;
        lap_var * boost * boost
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
pub(crate) const RESCAN_SAME_FACE_IOU: f32 = 0.3;

/// Maximum confidence for a RetinaFace detection inside a pet box to be treated
/// as an animal-face false positive rather than a real person.
pub(crate) const PET_FACE_FALSE_POSITIVE_MAX_CONFIDENCE: f32 = 0.75;

/// Intersection over union of two (x, y, width, height) boxes.
pub(crate) fn box_iou(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> f32 {
    let ix = (a.0 + a.2).min(b.0 + b.2) - a.0.max(b.0);
    let iy = (a.1 + a.3).min(b.1 + b.3) - a.1.max(b.1);
    if ix <= 0.0 || iy <= 0.0 {
        return 0.0;
    }
    let inter = ix * iy;
    let union = a.2 * a.3 + b.2 * b.3 - inter;
    if union <= 0.0 { 0.0 } else { inter / union }
}

/// Fraction of box `inner`'s area that lies inside `outer`.
pub(crate) fn box_containment(inner: (f32, f32, f32, f32), outer: (f32, f32, f32, f32)) -> f32 {
    let inner_area = inner.2 * inner.3;
    if inner_area <= 0.0 {
        return 0.0;
    }
    let ix = (inner.0 + inner.2).min(outer.0 + outer.2) - inner.0.max(outer.0);
    let iy = (inner.1 + inner.3).min(outer.1 + outer.3) - inner.1.max(outer.1);
    if ix <= 0.0 || iy <= 0.0 {
        return 0.0;
    }
    (ix * iy) / inner_area
}

/// RetinaFace occasionally fires with low-to-medium confidence on front-facing dogs/cats.
/// Drop such detections when they sit almost entirely inside a detected pet box.
fn is_human_false_positive_in_pet(face: &FaceBox, pets: &[Detection]) -> bool {
    if face.confidence >= PET_FACE_FALSE_POSITIVE_MAX_CONFIDENCE {
        return false;
    }
    let fbox = (face.x, face.y, face.width, face.height);
    pets.iter().any(|pet| {
        let pbox = (pet.bbox.x, pet.bbox.y, pet.bbox.width, pet.bbox.height);
        box_containment(fbox, pbox) >= 0.80
    })
}

/// Decode an upright image for face/pet detection: for images, prefers the local original
/// and falls back to the stored thumbnail; for videos, prefers the stored thumbnail (matching
/// the preview/cover frame) and falls back to extracting a frame from the local video.
pub(crate) fn decode_image_for_faces(file_id: i64, file_path: &str) -> Option<Vec<u8>> {
    let original_local = !crate::t_utils::is_cloud_only_path(file_path)
        && crate::t_utils::content_readable(file_path);
    if crate::t_utils::get_file_type(file_path) == Some(2) {
        return t_sqlite::AThumb::fetch(file_id)
            .ok()
            .flatten()
            .and_then(|thumb| thumb.thumb_data)
            .or_else(|| {
                if original_local {
                    crate::t_video::get_video_thumbnail_sync(
                        file_path,
                        t_common::FACE_DECODE_SIZE,
                        None,
                        None,
                    )
                    .ok()
                    .flatten()
                } else {
                    None
                }
            });
    }
    let decoded = if original_local {
        crate::t_image::decode_upright_for_faces(file_path, t_common::FACE_DECODE_SIZE)
    } else {
        None
    };
    decoded.or_else(|| {
        t_sqlite::AThumb::fetch(file_id)
            .ok()
            .flatten()
            .and_then(|thumb| thumb.thumb_data)
    })
}

/// Clockwise quarter turns (0..=3) of an in-app rotation (`afiles.rotate`, degrees clockwise).
/// The in-app rotation sits on top of the EXIF-upright decode, so detection has to apply
/// it too or a photo the user turned upright is still scanned sideways.
pub(crate) fn quarter_turns(rotate_degrees: i64) -> u32 {
    (rotate_degrees.rem_euclid(360) / 90) as u32
}

/// Rotate an image clockwise by `turns` quarter turns.
pub(crate) fn rotate_image(img: DynamicImage, turns: u32) -> DynamicImage {
    match turns % 4 {
        1 => img.rotate90(),
        2 => img.rotate180(),
        3 => img.rotate270(),
        _ => img,
    }
}

/// `decode_image_for_faces`, then turned by the in-app rotation so it matches what the
/// user sees (used to embed boxes the user drew or reclassified).
pub(crate) fn decode_image_for_faces_rotated(
    file_id: i64,
    file_path: &str,
    rotate_degrees: i64,
) -> Option<Vec<u8>> {
    let bytes = decode_image_for_faces(file_id, file_path)?;
    match quarter_turns(rotate_degrees) {
        0 => Some(bytes),
        turns => rotate_encoded(&bytes, turns),
    }
}

/// Rotate encoded image bytes clockwise by `turns` quarter turns, re-encoded as JPEG.
fn rotate_encoded(image_bytes: &[u8], turns: u32) -> Option<Vec<u8>> {
    let img = image::load_from_memory(image_bytes).ok()?;
    let rgb = rotate_image(img, turns).to_rgb8();
    let mut out = std::io::Cursor::new(Vec::new());
    rgb.write_to(&mut out, image::ImageFormat::Jpeg).ok()?;
    Some(out.into_inner())
}

/// Where box `(x, y, w, h)` in a `frame` of `(width, height)` lands after rotating the
/// frame clockwise by `turns` quarter turns. Works for pixel or normalized (1x1) frames.
pub(crate) fn rotate_box(
    b: (f32, f32, f32, f32),
    frame: (f32, f32),
    turns: u32,
) -> (f32, f32, f32, f32) {
    let (mut b, mut frame) = (b, frame);
    for _ in 0..turns % 4 {
        // One clockwise turn maps point (x, y) to (H - y, x) and swaps the frame.
        b = (frame.1 - (b.1 + b.3), b.0, b.3, b.2);
        frame = (frame.1, frame.0);
    }
    b
}

/// Detect faces and pets in an EXIF-upright decode, honoring the in-app rotation, and
/// return boxes in the stored upright frame `(width, height)` (falling back to the decoded
/// size when unknown). With `try_sideways`, a pass that finds no human faces is retried a
/// quarter turn either way, catching photos whose camera orientation tag is wrong.
pub(crate) fn detect_in_upright_frame(
    engine: &mut FaceEngine,
    image_bytes: &[u8],
    (width, height): (u32, u32),
    rotate_degrees: i64,
    try_sideways: bool,
) -> Result<Vec<Detection>, String> {
    let user_turns = quarter_turns(rotate_degrees);
    let mut passes = vec![user_turns];
    if try_sideways {
        passes.extend([(user_turns + 1) % 4, (user_turns + 3) % 4]);
    }

    let mut first_pass: Option<Vec<Detection>> = None;
    for turns in passes {
        let rotated;
        let bytes = if turns == 0 {
            image_bytes
        } else {
            rotated = rotate_encoded(image_bytes, turns)
                .ok_or_else(|| "Failed to rotate image for face detection".to_string())?;
            &rotated
        };
        let (mut detections, (proc_w, proc_h)) = engine.detect_all_from_bytes(bytes)?;
        if proc_w == 0 || proc_h == 0 {
            return Ok(Vec::new());
        }

        // Upright frame size, then the same frame as seen in this pass.
        let upright = if width > 0 && height > 0 {
            (width as f32, height as f32)
        } else if turns % 2 == 1 {
            (proc_h as f32, proc_w as f32)
        } else {
            (proc_w as f32, proc_h as f32)
        };
        let pass_frame = if turns % 2 == 1 { (upright.1, upright.0) } else { upright };
        let (sx, sy) = (pass_frame.0 / proc_w as f32, pass_frame.1 / proc_h as f32);
        for det in &mut detections {
            let b = &mut det.bbox;
            let scaled = (b.x * sx, b.y * sy, b.width * sx, b.height * sy);
            let (x, y, w, h) = rotate_box(scaled, pass_frame, (4 - turns) % 4);
            (b.x, b.y, b.width, b.height) = (x, y, w, h);
            // Landmarks are in this pass's decoded pixels and are not stored; drop them
            // rather than return them in a different frame from the box.
            b.landmarks = None;
        }

        if detections.iter().any(|d| d.kind == "face") {
            return Ok(detections);
        }
        first_pass.get_or_insert(detections);
    }
    Ok(first_pass.unwrap_or_default())
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
    // Use provided epsilon or the recommended default (calibrated on aligned MobileFaceNet embeddings)
    let epsilon = cluster_epsilon.unwrap_or(t_common::FACE_CLUSTER_THRESHOLD);

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

        for (file_id, file_path, width, height, rotate) in files {
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
            // Boxes come back in the stored upright frame, with the in-app rotation applied
            // for detection only.
            let process_result = detect_in_upright_frame(
                &mut engine,
                &image_bytes,
                (width.max(0) as u32, height.max(0) as u32),
                rotate,
                false,
            );
            drop(engine);

            match process_result {
                Ok(mut faces) => {

                    // Rescan: keep stored faces and pets (and their names); add only
                    // detections that don't overlap a stored or dismissed one of the same kind.
                    let mut already_had_faces = false;
                    if rescan {
                        let existing = t_sqlite::Face::bboxes_for_file_with_conn(&db_conn, file_id)
                            .unwrap_or_default();
                        let dismissed =
                            t_sqlite::Face::dismissed_bboxes_for_file_with_conn(&db_conn, file_id)
                                .unwrap_or_default();
                        already_had_faces = !existing.is_empty();
                        faces.retain(|face| {
                            let b = &face.bbox;
                            let overlaps = |(kind, (x, y, w, h)): &(String, (f32, f32, f32, f32))| {
                                kind == face.kind
                                    && box_iou((b.x, b.y, b.width, b.height), (*x, *y, *w, *h))
                                        >= RESCAN_SAME_FACE_IOU
                            };
                            !existing.iter().any(overlaps) && !dismissed.iter().any(overlaps)
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
                                face_data.kind,
                                face_data.species,
                            ) {
                                Ok(_) => {
                                    total_faces += 1;
                                    if face_data.kind == "pet" {
                                        let b = &face_data.bbox;
                                        let _ = t_sqlite::Face::delete_overlapping_unassigned_face_false_positives_with_conn(
                                            &db_conn,
                                            file_id,
                                            (b.x, b.y, b.width, b.height),
                                        );
                                    }
                                }
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

        // People and pets cluster separately (different embeddings and thresholds).
        let mut total_persons = 0;
        for (kind, threshold) in [
            (t_sqlite::PersonKind::Person, epsilon),
            (t_sqlite::PersonKind::Pet, t_common::PET_CLUSTER_THRESHOLD),
        ] {
            let cancel_token_cluster = cancel_token.clone();
            match t_cluster::cluster_faces(
                threshold,
                kind,
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
                Ok(count) => total_persons += count,
                Err(e) => eprintln!("Clustering {} failed: {}", kind.as_str(), e),
            }
        }
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
    fn in_app_rotation_maps_boxes_between_upright_and_rotated_frames() {
        assert_eq!(quarter_turns(0), 0);
        assert_eq!(quarter_turns(270), 3);
        assert_eq!(quarter_turns(-90), 3);
        assert_eq!(quarter_turns(450), 1);

        // A 400x300 upright frame turned once clockwise becomes 300x400: the top-left
        // corner box moves to the top-right.
        assert_eq!(rotate_box((0.0, 0.0, 40.0, 30.0), (400.0, 300.0), 1), (270.0, 0.0, 30.0, 40.0));
        assert_eq!(rotate_box((10.0, 20.0, 40.0, 30.0), (400.0, 300.0), 2), (350.0, 250.0, 40.0, 30.0));

        // Rotating by `t` then by the remaining turns returns the original box.
        let b = (12.0, 34.0, 56.0, 78.0);
        for t in 0..4 {
            let frame_after = if t % 2 == 1 { (300.0, 400.0) } else { (400.0, 300.0) };
            let there = rotate_box(b, (400.0, 300.0), t);
            assert_eq!(rotate_box(there, frame_after, (4 - t) % 4), b, "turns {t}");
        }
    }

    /// Detection on the user-rotated image must give the same upright box as rotating the
    /// box itself: rotate a real image, detect, and compare against the upright scan.
    #[test]
    fn rotated_pixels_and_rotated_boxes_agree() {
        // A bright 20x10 block at (60, 30) in an 200x100 image, located after rotation.
        let mut img = image::RgbImage::new(200, 100);
        for y in 30..40 { for x in 60..80 { img.put_pixel(x, y, image::Rgb([255, 255, 255])); } }
        for turns in 1..4 {
            let rotated = rotate_image(DynamicImage::ImageRgb8(img.clone()), turns).to_rgb8();
            let (w, h) = rotated.dimensions();
            let lit: Vec<(u32, u32)> = (0..h)
                .flat_map(|y| (0..w).map(move |x| (x, y)))
                .filter(|&(x, y)| rotated.get_pixel(x, y)[0] > 0)
                .collect();
            let (x0, y0) = (lit.iter().map(|p| p.0).min().unwrap(), lit.iter().map(|p| p.1).min().unwrap());
            let (x1, y1) = (lit.iter().map(|p| p.0).max().unwrap(), lit.iter().map(|p| p.1).max().unwrap());
            let found = (x0 as f32, y0 as f32, (x1 - x0 + 1) as f32, (y1 - y0 + 1) as f32);
            assert_eq!(rotate_box((60.0, 30.0, 20.0, 10.0), (200.0, 100.0), turns), found, "turns {turns}");
        }
    }

    #[test]
    fn blur_score_preserves_dim_sharp_faces_and_rejects_flat_dark_patches() {
        let engine = FaceEngine::new();
        let bbox = FaceBox {
            x: 0.0,
            y: 0.0,
            width: 64.0,
            height: 64.0,
            confidence: 0.85,
            landmarks: None,
        };

        // Dim sharp checkerboard-like face texture (luma 10 vs 26, std ~ 8)
        let mut dim_sharp = image::RgbImage::new(64, 64);
        for (x, y, p) in dim_sharp.enumerate_pixels_mut() {
            let v = if ((x / 4) + (y / 4)) % 2 == 0 { 10 } else { 26 };
            *p = image::Rgb([v, v, v]);
        }
        let dim_score = engine.calculate_blur_score(&DynamicImage::ImageRgb8(dim_sharp), &bbox);
        assert!(
            dim_score >= t_common::MIN_BLUR_SCORE,
            "dim sharp crop should exceed MIN_BLUR_SCORE, got {dim_score}"
        );

        // Flat dark noisy patch (luma 10..11) should still stay below MIN_BLUR_SCORE
        let mut flat_dark = image::RgbImage::new(64, 64);
        for (x, y, p) in flat_dark.enumerate_pixels_mut() {
            let v = if (x + y) % 7 == 0 { 11 } else { 10 };
            *p = image::Rgb([v, v, v]);
        }
        let flat_score = engine.calculate_blur_score(&DynamicImage::ImageRgb8(flat_dark), &bbox);
        assert!(
            flat_score < t_common::MIN_BLUR_SCORE,
            "flat dark patch should stay below MIN_BLUR_SCORE, got {flat_score}"
        );
    }

    #[test]
    fn box_iou_detects_the_same_face() {
        assert!((box_iou((0.0, 0.0, 10.0, 10.0), (0.0, 0.0, 10.0, 10.0)) - 1.0).abs() < 1e-6);
        assert!(box_iou((0.0, 0.0, 10.0, 10.0), (1.0, 1.0, 10.0, 10.0)) >= RESCAN_SAME_FACE_IOU);
        assert_eq!(box_iou((0.0, 0.0, 10.0, 10.0), (20.0, 0.0, 10.0, 10.0)), 0.0);
    }

    #[test]
    fn human_false_positive_inside_pet_is_suppressed_only_when_low_confidence() {
        let pet = Detection {
            bbox: FaceBox { x: 0.0, y: 100.0, width: 300.0, height: 200.0, confidence: 0.7, landmarks: None },
            embedding: vec![],
            low_quality: false,
            kind: "pet",
            species: Some("dog"),
        };
        // Low-confidence RetinaFace false positive inside dog box -> suppressed
        let dog_face_fp = FaceBox { x: 100.0, y: 120.0, width: 100.0, height: 100.0, confidence: 0.55, landmarks: None };
        assert!(is_human_false_positive_in_pet(&dog_face_fp, &[pet.clone()]));
        // High-confidence human face -> kept
        let strong_human = FaceBox { x: 100.0, y: 120.0, width: 100.0, height: 100.0, confidence: 0.85, landmarks: None };
        assert!(!is_human_false_positive_in_pet(&strong_human, &[pet.clone()]));
        // Human face above the dog -> kept
        let human_above = FaceBox { x: 100.0, y: 0.0, width: 80.0, height: 80.0, confidence: 0.60, landmarks: None };
        assert!(!is_human_false_positive_in_pet(&human_above, &[pet]));
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
            pet_model: None,
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
            pet_model: None,
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

    /// Embeds Apple-labeled pets (dataset.json from the pet evaluation) with the
    /// app's pipeline and reports same-pet vs different-pet similarity.
    /// `PET_DATASET=.../dataset.json cargo test --bin Lap pet_pipeline -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn pet_pipeline_separates_labeled_pets() {
        let records: Vec<serde_json::Value> =
            serde_json::from_str(&std::fs::read_to_string(std::env::var("PET_DATASET").unwrap()).unwrap()).unwrap();
        let models = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/models");
        let load = |name: &str| Session::builder().unwrap().commit_from_file(models.join(name)).unwrap();
        let mut engine = FaceEngine {
            detection_model: Some(load(t_common::DETECTION_MODEL)),
            embedding_model: Some(load(t_common::EMBEDDING_MODEL)),
            pet_model: Some(load(t_common::PET_EMBEDDING_MODEL)),
        };
        let mut embedded: Vec<(String, Vec<f32>)> = Vec::new();
        let mut per_name: std::collections::HashMap<String, usize> = Default::default();
        for r in &records {
            let name = r["name"].as_str().unwrap_or("").to_string();
            if name.is_empty() || per_name.get(&name).copied().unwrap_or(0) >= 8 {
                continue;
            }
            let path = r["path"].as_str().unwrap();
            let Some(bytes) = crate::t_image::decode_upright_for_faces(path, t_common::FACE_DECODE_SIZE) else { continue };
            let (dets, (w, h)) = engine.detect_all_from_bytes(&bytes).unwrap();
            // The detection containing Apple's face center is the labeled pet.
            let (cx, cy) = (r["face_center"][0].as_f64().unwrap() as f32 * w as f32, r["face_center"][1].as_f64().unwrap() as f32 * h as f32);
            if let Some(d) = dets.iter().find(|d| d.kind == "pet"
                && cx >= d.bbox.x && cx <= d.bbox.x + d.bbox.width && cy >= d.bbox.y && cy <= d.bbox.y + d.bbox.height)
            {
                *per_name.entry(name.clone()).or_default() += 1;
                embedded.push((name, d.embedding.clone()));
            }
        }
        let (mut same, mut diff) = (Vec::new(), Vec::new());
        for i in 0..embedded.len() {
            for j in i + 1..embedded.len() {
                let sim: f32 = embedded[i].1.iter().zip(&embedded[j].1).map(|(a, b)| a * b).sum();
                if embedded[i].0 == embedded[j].0 { same.push(sim) } else { diff.push(sim) }
            }
        }
        let mean = |v: &Vec<f32>| v.iter().sum::<f32>() / v.len() as f32;
        println!("pets {:?}", per_name);
        println!("mean similarity: same pet {:.3}, different pets {:.3}", mean(&same), mean(&diff));
        assert!(mean(&same) - mean(&diff) > 0.3);
    }
}
