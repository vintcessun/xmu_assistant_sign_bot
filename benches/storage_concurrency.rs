use criterion::{Criterion, criterion_group, criterion_main};
use rand::Rng;
use std::sync::Arc;
use tokio::runtime::Runtime;
use xmu_assistant_bot::api::storage::HotTable;

fn bench_storage_concurrency(c: &mut Criterion) {
    // 1. 创建异步运行时
    let rt = Runtime::new().unwrap();

    // 2. 关键修复：通过 rt.enter() 进入运行时上下文
    // 这会返回一个 guard，在 guard 存活期间，所有初始化代码都能找到 Tokio Reactor
    let guard = rt.enter();

    // 3. 在上下文保护内初始化 HotTable
    // 假设你的 HotTable 实现了某些内部异步同步逻辑，现在它可以安全获取线程局部的 Runtime 句柄了
    let table = Arc::new(HotTable::<String, String>::new("bench_test"));

    // 4. 预先填充数据，确保读取操作成功
    // 注意: HotTable 是内存缓存，这里使用 rt.block_on 进行同步初始化
    rt.block_on(async {
        for i in 0..100 {
            let key = format!("user_{}", i);
            table
                .insert(key.clone(), "initial_session_data".to_string().into())
                .expect("Initial insert failed");
        }
    });

    c.bench_function("hottable_concurrent_read_write_x100_90_10", |b| {
        b.to_async(&rt).iter(|| {
            let table = table.clone();
            let mut rng = rand::rng();

            async move {
                let mut tasks = vec![];

                // 模拟 100 个并发协程同时操作存储
                for i in 0..100 {
                    let t = table.clone();
                    // 90% 读 (0-8)，10% 写 (9)
                    let op_type = rng.random_range(0..10);
                    let key = format!("user_{}", i);

                    tasks.push(tokio::spawn(async move {
                        if op_type == 0 {
                            // 写 (10%)
                            t.insert(key.clone(), "updated_session_data".to_string().into())
                                .expect("Insert failed during bench");
                        } else {
                            // 读 (90%)
                            let _ = t.get(&key).expect("Read failed during bench");
                        }
                    }));
                }

                // 等待所有并发任务结束
                for task in tasks {
                    let _ = task.await;
                }
            }
        });
    });

    // 显式清理：由于 HotTable 在后台启动了永久任务，我们必须强制运行时关闭，
    // 否则 Criterion 将挂起。我们首先 drop 掉 guard，然后调用 shutdown_background。
    std::mem::drop(guard);
    rt.shutdown_background();
}

criterion_group!(benches, bench_storage_concurrency);
criterion_main!(benches);
