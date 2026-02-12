use crate::api::llm::chat::tool::python::monty::run_python_code;
use crate::api::llm::tool::LlmEnum;
use crate::api::llm::tool::{LlmBool, LlmF64, LlmHashMap, LlmI64, ask_as};
use crate::api::llm::tool::{LlmPrompt, LlmVec};
use anyhow::Result;
use genai::chat::ChatMessage;
use helper::LlmPrompt;
use helper::tool;
use monty::{DictPairs, MontyObject};
use serde::{Deserialize, Serialize};

//TODO: 去掉二次转发
//TODO: 对于结构体自动扫描文档并生成提示词

#[tool(
    description = "根据用户的需求生成 Python 代码并执行，最后返回结果。请根据用户的需求生成符合要求的 Python 代码，并且在最后调用函数时传入正确的参数。"
)]
pub async fn python_exec(
    /// 用户对要执行的 Python 代码的需求描述，代码必须符合以下格式（即函数应该在最后被调用，就像直接输入REPL进行运行一样）:
    description: String,
) -> Result<String> {
    let chat_msg = vec![
        ChatMessage::system(
            "你是一个 Python 代码生成和执行专家，根据用户的需求生成 Python 代码并执行，最后返回结果。请根据用户的需求生成符合要求的 Python 代码，并且在最后调用函数时传入正确的参数。",
        ),
        ChatMessage::system(
            r#"代码必须符合以下格式（即函数应该在最后被调用，就像直接输入REPL进行运行一样）:
def fib(n):
    if n <= 1:
        return n
    return fib(n - 1) + fib(n - 2)

fib(x)

比如对于这段代码要传入参数 x=12 其中 12为整数类型"#,
        ),
        ChatMessage::user(description),
    ];
    let code = ask_as::<PythonExecRequest>(chat_msg).await?;
    #[cfg(test)]
    println!("Generated Python Code: {:?}", code);
    let result = run_python_code(code).await?;
    Ok(result)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Ord, PartialOrd)]
#[serde(transparent)]
pub struct PythonValueWeak(PythonValue);

impl LlmPrompt for PythonValueWeak {
    fn get_prompt_schema() -> &'static str {
        "<PythonValue>类型声明如上文所示</PythonValue>"
    }
    fn root_name() -> &'static str {
        "PythonValue"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, LlmPrompt, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum PythonValue {
    #[prompt("python的None")]
    None,
    #[prompt("python的str")]
    String {
        #[prompt("字符串的值")]
        val: String,
    },
    #[prompt("python的int")]
    Int {
        #[prompt("整数的值")]
        val: LlmI64,
    },
    #[prompt("python的float")]
    Float {
        #[prompt("浮点数的值")]
        val: LlmF64,
    },
    #[prompt("python的bool")]
    Bool {
        #[prompt("布尔值的值")]
        val: LlmBool,
    },
    #[prompt("python的list")]
    List {
        #[prompt("列表的值")]
        val: LlmVec<PythonValueWeak>,
    },
    #[prompt("python的dict")]
    Dict {
        #[prompt("字典的值")]
        val: LlmHashMap<PythonValueWeak, PythonValueWeak>,
    },
}

pub fn python_value_to_monty_object(value: &PythonValue) -> MontyObject {
    match value {
        PythonValue::None => MontyObject::None,
        PythonValue::String { val } => MontyObject::String(val.trim().into()),
        PythonValue::Int { val } => MontyObject::Int(val.into()),
        PythonValue::Float { val } => MontyObject::Float(val.into()),
        PythonValue::Bool { val } => MontyObject::Bool(val.into()),
        PythonValue::List { val } => {
            let items = val
                .iter()
                .map(|x| python_value_to_monty_object(&x.0))
                .collect();
            MontyObject::List(items)
        }
        PythonValue::Dict { val } => {
            let mut pairs = Vec::new();
            for (k, v) in val.iter() {
                pairs.push((
                    python_value_to_monty_object(&k.0),
                    python_value_to_monty_object(&v.0),
                ));
            }
            MontyObject::Dict(DictPairs::from(pairs))
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, LlmPrompt)]
pub struct PythonParam {
    #[prompt("参数的名字，应该和代码中最后调用函数的参数名一致")]
    pub name: String,
    #[prompt("参数的值，应该是一个字符串，如果需要传入复杂数据结构请在代码中解析这个字符串")]
    pub value: LlmEnum<PythonValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize, LlmPrompt)]
pub struct PythonExecRequest {
    #[prompt("要执行脚本的名字以.py结尾")]
    pub script_name: String,
    #[prompt("要传入的参数列表")]
    pub params: LlmVec<PythonParam>,
    #[prompt(r#"要执行的 python 代码"#)]
    pub code: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_test() {
        println!("PythonValue Schema: {}", PythonValue::get_prompt_schema());
        println!("PythonParam Schema: {}", PythonParam::get_prompt_schema());
        println!(
            "PythonExecRequest Schema: {}",
            PythonExecRequest::get_prompt_schema()
        );
    }

    #[tokio::test]
    async fn test_parse_params() -> Result<()> {
        let src = r#"
      <PythonParam>
        <name>n</name>
        <value>
          <Int>
            <val>10</val>
          </Int>
        </value>
      </PythonParam>"#;
        let parsed: PythonParam = quick_xml::de::from_str(src)?;
        println!("Parsed PythonExecRequest: {:?}", parsed);
        Ok(())
    }

    #[tokio::test]
    async fn test_parse_request() -> Result<()> {
        let src = r#"<PythonExecRequest>
  <script_name>fibonacci.py</script_name>
  <params>
    <item>
      <PythonParam>
        <name>n</name>
        <value>
          <Int>
            <val>10</val>
          </Int>
        </value>
      </PythonParam>
    </item>
  </params>
  <code><![CDATA[
def fib(n):
    if n <= 1:
        return n
    return fib(n - 1) + fib(n - 2)

fib(n)]]>
  </code>
</PythonExecRequest>"#;
        let parsed: PythonExecRequest = quick_xml::de::from_str(src)?;
        println!("Parsed PythonExecRequest: {:?}", parsed);
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_python_tool() -> Result<()> {
        let tool = PythonExecTool::tool();
        println!("Tool Definition: {:?}", tool);
        let name = PythonExecTool::FN_NAME;
        println!("Tool Function Name: {}", name);
        let args = PythonExecArgs {
            description:
                "定义一个函数来计算斐波那契数列的第n个数字（假设F1=1, F2=1），并计算第10个数字。"
                    .to_string(),
        };
        println!("Tool Call Arguments: {:?}", args);
        let call_ret = PythonExecTool::call(args).await?;
        println!("Tool Call Result: {}", call_ret);
        Ok(())
    }
}
