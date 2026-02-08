use crate::api::llm::tool::LlmPrompt;

impl LlmPrompt for String {
    fn get_prompt_schema() -> &'static str {
        "返回String类型"
    }

    fn root_name() -> &'static str {
        "string"
    }
}
