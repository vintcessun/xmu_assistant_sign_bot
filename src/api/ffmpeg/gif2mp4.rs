use anyhow::{Result, anyhow};
use std::path::PathBuf;
use tokio::task::block_in_place;

// 导入 video-rs 的高层 API
use super::init_ffmpeg;
use video_rs::{
    decode::Decoder,
    encode::{Encoder, Settings},
    error::Error,
};

pub fn gif_to_mp4_silent(input_path: &PathBuf, output_path: &PathBuf) -> Result<()> {
    // 1. 初始化 FFmpeg
    init_ffmpeg();

    // 2. 设置解码器
    let mut decoder = Decoder::new(input_path.as_path())
        .map_err(|e| anyhow::anyhow!("Failed to create decoder for {:?}: {:?}", input_path, e))?;

    // 3. 获取视频尺寸和帧率 (使用 Decoder 提供的公共方法)
    let (width, height) = decoder.size(); // 输入尺寸

    // 4. 适配分辨率 (宽度和高度必须为偶数，以兼容 YUV420P)
    let out_width = width / 2 * 2;
    let out_height = height / 2 * 2;

    let out_width_usize = out_width as usize;
    let out_height_usize = out_height as usize;

    if out_width_usize == 0 || out_height_usize == 0 {
        anyhow::bail!(
            "Calculated dimensions are zero: {}x{}",
            out_width_usize,
            out_height_usize
        );
    }

    // 5. 编码设置
    let settings = Settings::preset_h264_yuv420p(out_width_usize, out_height_usize, false);

    // 6. 创建编码器
    let mut encoder = Encoder::new(output_path.as_path(), settings)
        .map_err(|e| anyhow::anyhow!("Failed to create encoder for {:?}: {:?}", output_path, e))?;

    // 7. 开始解码 (使用 decode_iter)
    let decoded_frames = decoder.decode_iter();

    // 8. 帧处理循环
    for result in decoded_frames {
        match result {
            Ok((time, frame)) => {
                encoder.encode(&frame, time)?;
            }
            Err(e) => {
                if let Error::DecodeExhausted = e {
                    // 流结束，退出循环
                    break;
                }
                return Err(anyhow!("Error decoding frame: {:?}", e));
            }
        }
    }

    // 9. 结束编码
    encoder.finish()?;

    Ok(())
}

pub async fn gif_to_mp4_silent_async(input_path: &PathBuf, output_path: &PathBuf) -> Result<()> {
    block_in_place(|| gif_to_mp4_silent(input_path, output_path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;

    fn test_file(id: i64) -> Result<()> {
        let input_path = PathBuf::from(format!("test_data/ffmpeg_gif_test_{}.gif", id));
        let output_path = PathBuf::from(format!("test_data/ffmpeg_gif_test_{}.mp4", id));

        gif_to_mp4_silent(&input_path, &output_path)?;

        // 验证文件存在
        assert!(output_path.exists());

        Ok(())
    }

    #[test]
    fn test_gif_to_mp4_silent() -> Result<()> {
        test_file(1)?;
        test_file(2)?;
        Ok(())
    }
}
