use crate::api::llm::tool::LlmPrompt;
use serde::{
    Serialize,
    de::{self, Unexpected, Visitor},
};
use std::{
    fmt,
    ops::{Add, Deref, DerefMut, Div, Mul, Rem, Sub},
};

#[derive(Debug, Clone, PartialEq, Serialize, Copy, Eq, Default)]
#[serde(transparent)]
pub struct LlmUsize(pub usize);

impl Deref for LlmUsize {
    type Target = usize;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for LlmUsize {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

// --- 2. 基础转换 (From/Into) ---
impl From<usize> for LlmUsize {
    fn from(v: usize) -> Self {
        LlmUsize(v)
    }
}

impl From<LlmUsize> for usize {
    fn from(v: LlmUsize) -> Self {
        v.0
    }
}

// --- 3. 算术运算支持 (转发给内部 usize) ---
macro_rules! impl_op {
    ($trait:ident, $method:ident) => {
        impl $trait for LlmUsize {
            type Output = Self;
            fn $method(self, rhs: Self) -> Self::Output {
                LlmUsize(self.0.$method(rhs.0))
            }
        }
        impl $trait<usize> for LlmUsize {
            type Output = Self;
            fn $method(self, rhs: usize) -> Self::Output {
                LlmUsize(self.0.$method(rhs))
            }
        }
    };
}

impl_op!(Add, add);
impl_op!(Sub, sub);
impl_op!(Mul, mul);
impl_op!(Div, div);
impl_op!(Rem, rem);

impl fmt::Display for LlmUsize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<'de> de::Deserialize<'de> for LlmUsize {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct LlmUsizeVisitor;

        impl<'de> Visitor<'de> for LlmUsizeVisitor {
            type Value = LlmUsize;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("an integer or a string representing an integer")
            }

            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E> {
                Ok(LlmUsize(v as usize))
            }

            // 情况 2：解析器给了字符串（LLM 的常态）
            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                // 这里就是你想要的：实现明显的解析意图
                v.trim()
                    .parse::<usize>()
                    .map(LlmUsize)
                    .map_err(|_| de::Error::invalid_value(Unexpected::Str(v), &self))
            }
        }

        deserializer.deserialize_u64(LlmUsizeVisitor)
    }
}

impl LlmPrompt for LlmUsize {
    fn get_prompt_schema() -> &'static str {
        "整数"
    }
    fn root_name() -> &'static str {
        "usize"
    }
}
