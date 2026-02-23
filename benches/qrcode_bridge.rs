use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use tokio::runtime::Runtime;
use xmu_assistant_bot::api::qrcode::{bridge::QrCodeDetector, task::process_image};

fn bench_qrcode_init(c: &mut Criterion) {
    c.bench_function("qrcode_detector_init", |b| {
        b.iter(|| {
            let _ = black_box(QrCodeDetector::new().unwrap());
        })
    });
}

fn bench_qrcode_decode_reused(c: &mut Criterion) {
    let mut detector = QrCodeDetector::new().unwrap();
    let img_data = std::fs::read("app_data/preload_qrcode.jpg").expect("Failed to read test image");

    c.bench_function("qrcode_decode_reused_detector", |b| {
        b.iter(|| {
            let _ = black_box(detector.decode_from_bytes(&img_data).unwrap());
        })
    });
}

fn bench_qrcode_full_cycle(c: &mut Criterion) {
    let img_data = std::fs::read("app_data/preload_qrcode.jpg").expect("Failed to read test image");

    c.bench_function("qrcode_full_init_and_decode", |b| {
        b.iter(|| {
            let mut detector = QrCodeDetector::new().unwrap();
            let _ = black_box(detector.decode_from_bytes(&img_data).unwrap());
        })
    });
}

fn bench_qrcode_full_cycle_preload(c: &mut Criterion) {
    let img_data = std::fs::read("app_data/preload_qrcode.jpg").expect("Failed to read test image");
    let mut detector = QrCodeDetector::new().unwrap();
    let _ = detector.decode_from_bytes(&img_data).unwrap(); // 预热一次

    c.bench_function("qrcode_full_init_and_decode_preloaded", |b| {
        b.iter_with_setup(
            || img_data.clone(),
            |data| {
                let _ = black_box(detector.decode_from_bytes(&data).unwrap());
            },
        )
    });
}

fn bench_qrcode_channel(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let guard = rt.enter();
    let img_data = std::fs::read("app_data/preload_qrcode.jpg").expect("Failed to read test image");

    c.bench_function("qrcode_full_init_and_decode_channel", |b| {
        b.to_async(&rt).iter(|| {
            let img = img_data.clone();
            async move {
                process_image(img).await.unwrap();
            }
        });
    });

    std::mem::drop(guard);
    rt.shutdown_background();
}

criterion_group!(
    benches,
    bench_qrcode_init,
    bench_qrcode_decode_reused,
    bench_qrcode_full_cycle,
    bench_qrcode_full_cycle_preload,
    bench_qrcode_channel,
);
criterion_main!(benches);
