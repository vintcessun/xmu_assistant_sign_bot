const BASE: &str = "wechat_qrcode";

use crate::config::{DATA_DIR, ensure_dir};
use anyhow::{Context, Result, anyhow};
use const_format::concatcp;
use opencv::{core::Vector, imgcodecs, prelude::*, wechat_qrcode::WeChatQRCode};
use std::fs;
use std::path::Path;
use tracing::info;

include!(concat!(env!("OUT_DIR"), "/qrcode_data.rs"));

pub struct QrCodeDetector {
    detector: WeChatQRCode,
}

impl QrCodeDetector {
    pub fn new() -> Result<Self> {
        let path = concatcp!(DATA_DIR, "/", BASE);
        info!(path = path, "初始化QRCode模型文件目录");
        ensure_dir(path);
        let model_dir = Path::new(path);

        let detect_prototxt = model_dir.join("detect.prototxt");
        let detect_caffemodel = model_dir.join("detect.caffemodel");
        let sr_prototxt = model_dir.join("sr.prototxt");
        let sr_caffemodel = model_dir.join("sr.caffemodel");

        // 如果模型文件不存在，则从内嵌数据写入
        if !detect_prototxt.exists() {
            fs::write(&detect_prototxt, DETECT_PROTOTXT)
                .context("Failed to write detect.prototxt")?;
        }
        if !detect_caffemodel.exists() {
            fs::write(&detect_caffemodel, DETECT_CAFFEMODEL)
                .context("Failed to write detect.caffemodel")?;
        }
        if !sr_prototxt.exists() {
            fs::write(&sr_prototxt, SR_PROTOTXT).context("Failed to write sr.prototxt")?;
        }
        if !sr_caffemodel.exists() {
            fs::write(&sr_caffemodel, SR_CAFFEMODEL).context("Failed to write sr.caffemodel")?;
        }

        let detector = WeChatQRCode::new(
            &detect_prototxt.to_string_lossy(),
            &detect_caffemodel.to_string_lossy(),
            &sr_prototxt.to_string_lossy(),
            &sr_caffemodel.to_string_lossy(),
        )
        .map_err(|e| anyhow!("Failed to initialize WeChatQRCode: {}", e))?;

        Ok(Self { detector })
    }

    pub fn decode_from_file<P: AsRef<Path>>(&mut self, path: P) -> Result<Vec<String>> {
        let path_str = path.as_ref().to_string_lossy();
        let img = imgcodecs::imread(&path_str, imgcodecs::IMREAD_COLOR)
            .with_context(|| format!("Failed to read image from {}", path_str))?;

        self.decode_img(&img, &path_str)
    }

    pub fn decode_from_bytes(&mut self, bytes: &[u8]) -> Result<Vec<String>> {
        let img_vec = Vector::<u8>::from_iter(bytes.to_vec());
        let img = imgcodecs::imdecode(&img_vec, imgcodecs::IMREAD_COLOR)
            .context("Failed to decode image from bytes")?;

        self.decode_img(&img, "bytes")
    }

    fn decode_img(&mut self, img: &Mat, source: &str) -> Result<Vec<String>> {
        if img.empty() {
            return Err(anyhow!("Image from {} is empty", source));
        }

        let mut points = Vector::<Mat>::new();
        let res = self
            .detector
            .detect_and_decode(img, &mut points)
            .map_err(|e| anyhow!("Detection failed: {}", e))?;

        let decoded: Vec<String> = res.into_iter().collect();

        Ok(decoded)
    }

    pub fn preload(&mut self) -> Result<()> {
        self.decode_from_bytes(PRELOAD_QRCODE_JPG)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preload_qrcode() {
        let mut detector = QrCodeDetector::new().expect("Should be able to initialize detector");
        let results = detector
            .decode_from_bytes(PRELOAD_QRCODE_JPG)
            .expect("Should be able to run decoding");

        println!("Decoded results: {:?}", results);
        assert!(!results.is_empty(), "Should detect at least one QR code");
        assert!(
            !results[0].is_empty(),
            "Decoded content should not be empty"
        );
    }
}
