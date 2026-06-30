///! 这个是 config.rs 的模板 务必按照这个方式暴露 MODEL_MAP变量和 ModelConfig 结构体 然后按照自己配置就行了
use genai::adapter::AdapterKind;
use std::collections::HashMap;
use std::sync::LazyLock;

pub struct ModelConfig {
    pub kind: AdapterKind,
    pub base_url: &'static str,
    pub api_key_env: &'static str,
}

// 集中管理用于必要 LLM 选择的模型端点和厂商映射
pub static MODEL_MAP: LazyLock<HashMap<&'static str, ModelConfig>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    m.insert(
        "deepseek-chat",
        ModelConfig {
            kind: AdapterKind::OpenAI,
            base_url: "https://api.deepseek.com/v1/",
            // 优先从环境变量读取，否则将该字符串本身作为 Key。
            api_key_env: "DEEPSEEK_API_KEY",
        },
    );
    m
});
