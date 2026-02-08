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
pub struct LlmI64(pub i64);

impl Deref for LlmI64 {
    type Target = i64;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for LlmI64 {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

// --- 2. 基础转换 (From/Into) ---
impl From<i64> for LlmI64 {
    fn from(v: i64) -> Self {
        LlmI64(v)
    }
}

impl From<LlmI64> for i64 {
    fn from(v: LlmI64) -> Self {
        v.0
    }
}

// --- 3. 算术运算支持 (转发给内部 i64) ---
macro_rules! impl_op {
    ($trait:ident, $method:ident) => {
        impl $trait for LlmI64 {
            type Output = Self;
            fn $method(self, rhs: Self) -> Self::Output {
                LlmI64(self.0.$method(rhs.0))
            }
        }
        impl $trait<i64> for LlmI64 {
            type Output = Self;
            fn $method(self, rhs: i64) -> Self::Output {
                LlmI64(self.0.$method(rhs))
            }
        }
    };
}

impl_op!(Add, add);
impl_op!(Sub, sub);
impl_op!(Mul, mul);
impl_op!(Div, div);
impl_op!(Rem, rem);

impl fmt::Display for LlmI64 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<'de> de::Deserialize<'de> for LlmI64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct LlmI64Visitor;

        impl<'de> Visitor<'de> for LlmI64Visitor {
            type Value = LlmI64;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("an integer or a string representing an integer")
            }

            // 情况 1：解析器直接给了数字（原本咋样就咋样）
            fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E> {
                Ok(LlmI64(v))
            }

            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E> {
                Ok(LlmI64(v as i64))
            }

            // 情况 2：解析器给了字符串（LLM 的常态）
            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                // 这里就是你想要的：实现明显的解析意图
                v.trim()
                    .parse::<i64>()
                    .map(LlmI64)
                    .map_err(|_| de::Error::invalid_value(Unexpected::Str(v), &self))
            }
        }

        deserializer.deserialize_i64(LlmI64Visitor)
    }
}

impl LlmPrompt for LlmI64 {
    fn get_prompt_schema() -> &'static str {
        "整数"
    }
    fn root_name() -> &'static str {
        "i64"
    }
}
