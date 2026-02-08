use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};

use xmu_assistant_bot::abi::message::{MessageSend, message_body::MessageReceive};

// 包含多种类型段落的复杂消息 JSON 字符串
const COMPLEX_MESSAGE_JSON: &str = r#"[
        {
            "type": "text",
            "data": {
                "text": "这是一个测试文本，包含中文和一些符号！"
            }
        },
        {
            "type": "image",
            "data": {
                "file": "7a3036e4-411a-4c28-98e6-791784c98f80.png",
                "type": "flash",
                "url": "http://127.0.0.1:5000/image/7a3036e4-411a-4c28-98e6-791784c98f80.png"
            }
        },
        {
            "type": "at",
            "data": {
                "qq": "123456789"
            }
        },
        {
            "type": "share",
            "data": {
                "url": "https://example.com/share",
                "title": "分享标题",
                "content": "分享内容描述",
                "image": "https://example.com/image.jpg"
            }
        }
    ]"#;

// 辅助函数：反序列化一次以获取用于序列化测试的对象
fn get_message_send_object() -> MessageSend {
    serde_json::from_str(COMPLEX_MESSAGE_JSON).expect("Failed to deserialize mock JSON")
}

fn bench_serde_json_deserialization(c: &mut Criterion) {
    c.bench_function("json_deserialize_message_receive", |b| {
        b.iter(|| {
            let _: MessageReceive = serde_json::from_str(black_box(COMPLEX_MESSAGE_JSON)).unwrap();
        })
    });
}

fn bench_serde_json_serialization(c: &mut Criterion) {
    let msg_obj = get_message_send_object();
    c.bench_function("json_serialize_message_send", |b| {
        b.iter(|| {
            let _ = serde_json::to_string(black_box(&msg_obj)).unwrap();
        })
    });
}

// 测试 MessageReceive::get_text() 的性能
fn bench_message_get_text(c: &mut Criterion) {
    // 假设一个消息包含多个文本段。MessageReceive::Array 期望输入是一个数组。
    const ARRAY_TEXT_MESSAGE: &str = r#"[
            {"type": "text", "data": {"text": "Part 1 "}},
            {"type": "image", "data": {"file": "f.png", "url": "u"}},
            {"type": "text", "data": {"text": "Part 2 "}},
            {"type": "at", "data": {"qq": "123"}},
            {"type": "text", "data": {"text": "Part 3"}}
        ]"#;

    let msg_array: MessageReceive = serde_json::from_str(ARRAY_TEXT_MESSAGE).unwrap();

    // 假设一个单文本消息。MessageReceive::Single 期望输入是一个 Segment 对象。
    // SegmentReceive 是 internally tagged (tag="type", content="data")
    const SINGLE_TEXT_MESSAGE: &str = r#"{
        "type": "text", "data": {"text": "Simple text message"}
    }"#;
    let msg_single: MessageReceive = serde_json::from_str(SINGLE_TEXT_MESSAGE).unwrap();

    c.bench_function("get_text_array", |b| {
        b.iter(|| black_box(msg_array.get_text()))
    });

    c.bench_function("get_text_single", |b| {
        b.iter(|| black_box(msg_single.get_text()))
    });
}

criterion_group!(
    benches,
    bench_serde_json_deserialization,
    bench_serde_json_serialization,
    bench_message_get_text
);
criterion_main!(benches);
