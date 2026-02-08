///! 这个是 config.rs 的模板 务必按照这个方式暴露 MODEL_MAP变量和 ModelConfig 结构体 然后按照自己配置就行了
use genai::adapter::AdapterKind;
use std::collections::HashMap;
use std::sync::LazyLock;

pub struct ModelConfig {
    pub kind: AdapterKind,
    pub base_url: &'static str,
    pub api_key_env: &'static str,
}

// 在这里集中管理所有模型的端点和厂商映射
pub static MODEL_MAP: LazyLock<HashMap<&'static str, ModelConfig>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    m.insert(
        "text-embedding-3-large",
        ModelConfig {
            kind: AdapterKind::OpenAI,
            base_url: "your_base_url_here",
            api_key_env: "your_api_key_env_here",
        },
    );
    m.insert(
        "gemini-flash-latest",
        ModelConfig {
            kind: AdapterKind::Gemini,
            base_url: "your_base_url_here",
            api_key_env: "your_api_key_env_here",
        },
    );
    m
});
