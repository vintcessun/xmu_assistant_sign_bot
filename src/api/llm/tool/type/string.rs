use crate::api::llm::tool::LlmPrompt;

impl LlmPrompt for String {
    fn get_prompt_schema() -> &'static str {
        "返回String类型，请使用<![CDATA[{中间是实际的没有转义的字符串内容}]]>的格式来返回字符串内容，注意CDATA标签必须完全按照这个格式来，否则解析会失败。如果需要返回空字符串，请直接返回<![CDATA[]]>"
    }

    fn root_name() -> &'static str {
        "string"
    }
}
