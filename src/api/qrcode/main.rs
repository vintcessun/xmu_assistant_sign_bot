use crate::api::qrcode::task::process_image;
use anyhow::Result;

pub struct QrCode;

impl QrCode {
    pub async fn from_bytes(bytes: Vec<u8>) -> Result<Vec<String>> {
        process_image(bytes).await
    }

    pub async fn from_file<P: AsRef<std::path::Path>>(path: P) -> Result<Vec<String>> {
        let img_data = std::fs::read(path.as_ref())?;
        process_image(img_data).await
    }
}
