use criterion::{Criterion, criterion_group, criterion_main};
use std::sync::Arc;
use tokio::runtime::Runtime;
use xmu_assistant_bot::api::storage::ColdTable;

const TEST_TABLE_NAME: &str = "cold_bench_table";

fn setup_cold_table(rt: &Runtime) -> Arc<ColdTable<String, String>> {
    let _guard = rt.enter();
    Arc::new(ColdTable::new(TEST_TABLE_NAME))
}

fn bench_cold_storage(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let table = setup_cold_table(&rt);

    // --- 1. 插入性能测试 ---
    c.bench_function("cold_insert", |b| {
        let mut i = 0;
        b.to_async(&rt).iter(|| {
            let t = table.clone();
            let key = format!("key_{}", i);
            let value = format!("value_data_long_enough_to_test_bincode_{}", i);
            i += 1;
            // 循环使用 key，防止数据库无限膨胀
            if i > 10000 {
                i = 0;
            }

            async move {
                t.insert(key, value).await.unwrap();
            }
        });
    });

    // --- 2. 读取性能测试 ---
    // 预先插入 10000 条数据确保 get 能命中
    rt.block_on(async {
        for j in 0..10000 {
            let key = format!("key_{}", j);
            let value = format!("value_data_long_enough_to_test_bincode_{}", j);
            table.insert(key, value).await.unwrap();
        }
    });

    c.bench_function("cold_get_hit", |b| {
        let mut i = 0;
        b.to_async(&rt).iter(|| {
            let t = table.clone();
            let key = format!("key_{}", i);
            i = (i + 1) % 10000;

            async move {
                t.get_async(key).await.unwrap();
            }
        });
    });
}

criterion_group!(benches, bench_cold_storage);
criterion_main!(benches);
