use std::sync::LazyLock;
use video_rs::init;

static FF_INIT: LazyLock<()> = LazyLock::new(|| init().expect("初始化 ffmpeg 失败"));

pub fn init_ffmpeg() {
    // 触发 FFmpeg 库的初始化
    LazyLock::force(&FF_INIT);
}
