mod bool;
mod hashmap;
mod i64;
mod option;
mod string;
mod usize;
mod vector;

// 关于为什么要自己定义包装原有的各种类型，因为LLM本质还是概率模型，因此就会出现各种不严谨的情况，这很正常，但是我还是要喷一下
// 想起python有个库叫做json_repair，专门用来修复各种不规范的json格式，几把这种东西出现还是有道理的，特别是带着上下文回去给模型再来一次更是重量级
// 不如我把常见的情况手动修复了，要不然这个鸡毛东西就老是出错，搞得我都不敢用它了
// 最后：LLM真鸡巴操蛋，这里为了解析完全正确只能加一堆特判了，性能暂时不考虑了，因为操蛋的都寄吧LLM了，重新请求一次的代价远高于这些解析的开销

pub use bool::LlmBool;
pub use hashmap::LlmHashMap;
pub use i64::LlmI64;
pub use option::LlmOption;
pub use usize::LlmUsize;
pub use vector::LlmVec;
