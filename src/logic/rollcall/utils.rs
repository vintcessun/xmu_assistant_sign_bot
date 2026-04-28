use rand::RngExt;
use rand::distr::uniform::SampleRange;
use rand::distr::uniform::SampleUniform;
use std::cmp;
use std::time::{SystemTime, UNIX_EPOCH};

const TEMPLATE: &[u8] = b"xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx";
const HEX: &[u8] = b"0123456789abcdef";

macro_rules! generate_uuid_macro {
    ($template:expr, $rng:ident, $result:ident, { $($arms:tt)* }) => {
        {
            // 定义一个只负责处理单步逻辑的内部宏
            macro_rules! expand_step {
                ($i:expr) => {
                    {
                        let char = &$template[$i];
                        // 这里的代码块直接返回 u8 写入 buf[$i]
                        $result[$i] = match *char {
                            $($arms)*
                            _ => *char, // 默认情况直接使用模板字符
                        };
                    }
                };
            }

            // 手动或递归展开 36 次调用
            expand_step!(0);
            expand_step!(1);
            expand_step!(2);
            expand_step!(3);
            expand_step!(4);
            expand_step!(5);
            expand_step!(6);
            expand_step!(7);
            expand_step!(8);
            expand_step!(9);
            expand_step!(10);
            expand_step!(11);
            expand_step!(12);
            expand_step!(13);
            expand_step!(14);
            expand_step!(15);
            expand_step!(16);
            expand_step!(17);
            expand_step!(18);
            expand_step!(19);
            expand_step!(20);
            expand_step!(21);
            expand_step!(22);
            expand_step!(23);
            expand_step!(24);
            expand_step!(25);
            expand_step!(26);
            expand_step!(27);
            expand_step!(28);
            expand_step!(29);
            expand_step!(30);
            expand_step!(31);
            expand_step!(32);
            expand_step!(33);
            expand_step!(34);
            expand_step!(35);
        }
    };
}

/// UUID生成函数
pub fn generate_uuid() -> String {
    let mut result: [u8; 36] = [0; 36];
    let mut rng = rand::rng();

    generate_uuid_macro!(TEMPLATE, rng, result, {
        b'x' => {
            let e = rng.random_range(0..16);
            HEX[e as usize]
        }
        b'y' => {
            let e = rng.random_range(0..16);
            let r = (e & 3) | 8;
            HEX[r as usize]
        }
    });

    unsafe { String::from_utf8_unchecked(result.to_vec()) }
}

pub fn get_ts() -> u128 {
    // 获取当前系统时间
    let now = SystemTime::now();
    // 计算从UNIX纪元到现在的持续时间
    let duration = now.duration_since(UNIX_EPOCH).expect("时间计算错误");
    // 转换为毫秒并返回
    duration.as_millis()
}

pub fn uniform<T: SampleUniform, R: SampleRange<T>>(range: R) -> T {
    let mut rng = rand::rng();
    rng.random_range(range)
}

/// 计算两个字符串的 Levenshtein 相似度 (0.0 ~ 1.0)
pub fn string_similarity(s1: &str, s2: &str) -> f64 {
    let len1 = s1.chars().count();
    let len2 = s2.chars().count();

    if len1 == 0 && len2 == 0 {
        return 1.0;
    }
    if len1 == 0 || len2 == 0 {
        return 0.0;
    }

    // 确保 len1 是较短的那个，以进一步优化空间
    if len1 > len2 {
        return string_similarity(s2, s1);
    }

    // 空间优化：只需要两行数据，甚至可以优化为单行
    let mut column: Vec<usize> = (0..=len1).collect();

    let s2_chars: Vec<char> = s2.chars().collect();
    let s1_chars: Vec<char> = s1.chars().collect();

    for j in 1..=len2 {
        let mut last_diag = column[0];
        column[0] = j;
        for i in 1..=len1 {
            let old_col = column[i];
            let cost = if s1_chars[i - 1] == s2_chars[j - 1] {
                0
            } else {
                1
            };

            column[i] = cmp::min(cmp::min(column[i] + 1, column[i - 1] + 1), last_diag + cost);
            last_diag = old_col;
        }
    }

    let dist = column[len1] as f64;
    let max_len = cmp::max(len1, len2) as f64;

    // 转换为相似度百分比
    1.0 - (dist / max_len)
}
