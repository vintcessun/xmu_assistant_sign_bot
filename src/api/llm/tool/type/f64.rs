use crate::api::llm::tool::LlmPrompt;
use ordered_float::OrderedFloat;
use serde::{
    Serialize,
    de::{self, Unexpected, Visitor},
};
use std::{
    fmt,
    ops::{Add, Deref, DerefMut, Div, Mul, Rem, Sub},
};

#[derive(Debug, Clone, PartialEq, Serialize, Copy, Default, Hash, Eq, Ord, PartialOrd)]
#[serde(transparent)]
pub struct LlmF64(pub OrderedFloat<f64>);

impl Deref for LlmF64 {
    type Target = f64;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for LlmF64 {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

// --- 2. 基础转换 (From/Into) ---
impl From<f64> for LlmF64 {
    fn from(v: f64) -> Self {
        LlmF64(OrderedFloat(v))
    }
}

impl From<LlmF64> for f64 {
    fn from(v: LlmF64) -> Self {
        v.0.into()
    }
}

impl From<&LlmF64> for f64 {
    fn from(v: &LlmF64) -> Self {
        v.0.into()
    }
}

// --- 3. 算术运算支持 (转发给内部 f64) ---
macro_rules! impl_op {
    ($trait:ident, $method:ident) => {
        impl $trait for LlmF64 {
            type Output = Self;
            fn $method(self, rhs: Self) -> Self::Output {
                LlmF64(self.0.$method(rhs.0))
            }
        }
        impl $trait<f64> for LlmF64 {
            type Output = Self;
            fn $method(self, rhs: f64) -> Self::Output {
                LlmF64(self.0.$method(rhs))
            }
        }
    };
}

impl_op!(Add, add);
impl_op!(Sub, sub);
impl_op!(Mul, mul);
impl_op!(Div, div);
impl_op!(Rem, rem);

impl fmt::Display for LlmF64 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<'de> de::Deserialize<'de> for LlmF64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        struct LlmF64Visitor;

        impl<'de> Visitor<'de> for LlmF64Visitor {
            type Value = LlmF64;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a float or a string representing a float")
            }

            // 情况 1：解析器直接给了数字（原本咋样就咋样）
            fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E> {
                Ok(LlmF64(OrderedFloat(v)))
            }

            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E> {
                Ok(LlmF64(OrderedFloat(v as f64)))
            }

            // 情况 2：解析器给了字符串（LLM 的常态）
            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                // 这里就是你想要的：实现明显的解析意图
                v.trim()
                    .parse::<f64>()
                    .map(|x| LlmF64(OrderedFloat(x)))
                    .map_err(|_| de::Error::invalid_value(Unexpected::Str(v), &self))
            }
        }

        deserializer.deserialize_f64(LlmF64Visitor)
    }
}

impl LlmPrompt for LlmF64 {
    fn get_prompt_schema() -> &'static str {
        "小数"
    }
    fn root_name() -> &'static str {
        "f64"
    }
}
