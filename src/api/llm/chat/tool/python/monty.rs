use std::time::Duration;

use crate::api::llm::chat::tool::python::{
    PythonExecRequest, PythonParam, python_value_to_monty_object,
};
use anyhow::Result;
use monty::{LimitedTracker, MontyRun, PrintWriter, ResourceLimits};
use tokio::task::block_in_place;

pub async fn run_python_code(request: PythonExecRequest) -> Result<String> {
    block_in_place(|| {
        let PythonExecRequest {
            script_name,
            params,
            code,
        } = request;
        let script_name_clean = script_name.trim();
        let code_clean = code.trim();
        let mut input_names = Vec::with_capacity(params.len());
        let mut input_values = Vec::with_capacity(params.len());
        for param in params {
            let PythonParam { name, value } = param;
            input_names.push(name.trim().to_string());
            input_values.push(python_value_to_monty_object(&value));
        }

        #[cfg(test)]
        println!("运行代码: \n{}", code_clean);
        let runner = MontyRun::new(code_clean.to_string(), script_name_clean, input_names)?;
        let mut collect_str = String::with_capacity(128);
        let collect = PrintWriter::Collect(&mut collect_str);
        let result = runner.run(
            input_values,
            LimitedTracker::new(ResourceLimits::new().max_duration(Duration::from_mins(20))),
            collect,
        )?;
        Ok(format!("stdout:\n{}\n返回值:\n{}", collect_str, result))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use monty::{MontyObject, NoLimitTracker, PrintWriter};

    #[test]
    fn test_exec() {
        let code = r#"
def fib(n):
    if n <= 1:
        return n
    return fib(n - 1) + fib(n - 2)

fib(x)
"#;

        let runner = MontyRun::new(code.to_owned(), "fib.py", vec!["x".to_owned()]).unwrap();
        let result = runner
            .run(
                vec![MontyObject::Int(10)],
                NoLimitTracker,
                PrintWriter::Stdout,
            )
            .unwrap();
        println!("捕获到的结果: {}", result);
        assert_eq!(result, MontyObject::Int(55));
    }

    #[test]
    fn test_serialize() {
        // Serialize parsed code
        let runner = MontyRun::new("x + 1".to_owned(), "main.py", vec!["x".to_owned()]).unwrap();
        let bytes = runner.dump().unwrap();

        // Later, restore and run
        let runner2 = MontyRun::load(&bytes).unwrap();
        let result = runner2
            .run(
                vec![MontyObject::Int(41)],
                NoLimitTracker,
                PrintWriter::Stdout,
            )
            .unwrap();
        println!("捕获到的结果: {:?}", result);
        assert_eq!(result, MontyObject::Int(42));
    }

    #[test]
    fn test_duck() {
        let code = r#"
def add(a, b):
    return a + b

add(x, y)
"#;
        let runner = MontyRun::new(
            code.to_owned(),
            "duck.py",
            vec!["x".to_owned(), "y".to_owned()],
        )
        .unwrap();
        let result = runner
            .run(
                vec![MontyObject::Int(10), MontyObject::Int(20)],
                NoLimitTracker,
                PrintWriter::Stdout,
            )
            .unwrap();
        println!("捕获到的结果: {:?}", result);
        assert_eq!(result, MontyObject::Int(30));

        let result = runner
            .run(
                vec![
                    MontyObject::String("Hello, ".to_owned()),
                    MontyObject::String("world!".to_owned()),
                ],
                NoLimitTracker,
                PrintWriter::Stdout,
            )
            .unwrap();
        println!("捕获到的结果: {:?}", result);
        assert_eq!(result, MontyObject::String("Hello, world!".to_owned()));
    }

    #[test]
    fn test_error_handling() {
        let code = r#"
def div(a, b):
    return a / b

div(x, y)
"#;
        let runner = MontyRun::new(
            code.to_owned(),
            "div.py",
            vec!["x".to_owned(), "y".to_owned()],
        )
        .unwrap();
        let result = runner.run(
            vec![MontyObject::Int(10), MontyObject::Int(0)],
            NoLimitTracker,
            PrintWriter::Stdout,
        );
        println!("捕获到的结果: {:?}", result);
        assert!(result.is_err());
    }

    #[test]
    fn test_json_parse() {
        // 到2026.3.16为止 monty 仍然没有支持 json 的功能
        let code = r#"
import json
def parse_json(s):
    return json.loads(s)

parse_json(x)
"#;
        let runner = MontyRun::new(code.to_owned(), "json_parse.py", vec!["x".to_owned()]).unwrap();
        let result = runner.run(
            vec![MontyObject::String(
                r#"{"key": "value", "num": 42}"#.to_owned(),
            )],
            NoLimitTracker,
            PrintWriter::Stdout,
        );
        println!("捕获到的结果: {:?}", result);
        assert!(result.is_err());
    }

    #[test]
    fn test_stdout() {
        let mut collect_str = String::new();
        let collect = PrintWriter::Collect(&mut collect_str);
        let code = r#"
def fib(n):
    if n <= 1:
        return n
    return fib(n - 1) + fib(n - 2)

result = fib(x)
print("Fibonacci result is:", result)
"#;
        let runner = MontyRun::new(code.to_owned(), "fib.py", vec!["x".to_owned()]).unwrap();
        let result = runner
            .run(vec![MontyObject::Int(10)], NoLimitTracker, collect)
            .unwrap();
        println!("捕获到的结果: {}", result);
        println!("捕获到的输出: {}", collect_str);
        assert_eq!(result, MontyObject::None);
        assert_eq!(collect_str.trim(), "Fibonacci result is: 55");
    }
}
