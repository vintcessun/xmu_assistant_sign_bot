mod bool;
mod r#enum;
mod f64;
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
use dashmap::DashMap;
pub use r#enum::LlmEnum;
pub use f64::LlmF64;
pub use hashmap::LlmHashMap;
pub use i64::LlmI64;
pub use option::LlmOption;
pub use usize::LlmUsize;
pub use vector::LlmVec;

// 因为沟槽的多态 static 无法单态化可能被 ICF 错误地合并，导致不同类型的提示词 schema 冲突
// 这里使用了一些技巧来避免这个问题

use std::{
    any::TypeId,
    sync::{LazyLock, OnceLock},
};

pub struct CacheInner {
    pub prompt_schema: OnceLock<String>,
    pub root_name: OnceLock<String>,
}

static CACHE_HOLDER: LazyLock<DashMap<TypeId, &'static CacheInner>> = LazyLock::new(DashMap::new);

pub struct Cache<T>(std::marker::PhantomData<T>);

impl<T: 'static> Cache<T> {
    /// 获取属于类型 T 的缓存对象
    pub fn get() -> &'static CacheInner {
        let tid = TypeId::of::<T>();

        if let Some(inner) = CACHE_HOLDER.get(&tid) {
            return *inner;
        }

        *CACHE_HOLDER.entry(tid).or_insert_with(|| {
            Box::leak(Box::new(CacheInner {
                prompt_schema: OnceLock::new(),
                root_name: OnceLock::new(),
            }))
        })
    }
}

static CACHE_PAIR_HOLDER: LazyLock<DashMap<(TypeId, TypeId), &'static CacheInner>> =
    LazyLock::new(DashMap::new);

pub struct CachePair<K, V>(std::marker::PhantomData<K>, std::marker::PhantomData<V>);

impl<K: 'static, V: 'static> CachePair<K, V> {
    /// 获取属于类型 T 的缓存对象
    pub fn get() -> &'static CacheInner {
        let tid_k = TypeId::of::<K>();
        let tid_v = TypeId::of::<V>();
        let key = (tid_k, tid_v);

        if let Some(inner) = CACHE_PAIR_HOLDER.get(&key) {
            return *inner;
        }

        *CACHE_PAIR_HOLDER.entry(key).or_insert_with(|| {
            Box::leak(Box::new(CacheInner {
                prompt_schema: OnceLock::new(),
                root_name: OnceLock::new(),
            }))
        })
    }
}
