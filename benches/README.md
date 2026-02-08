# Benchmarking Guidelines & Safety Rules

## ⚠️ 核心警告：内存安全与类型欺骗

本项目中的 `#[handler]` 过程宏为了实现泛型路由，在内部使用了 `std::mem::transmute`。这是一种极其高效但危险的操作，它要求 **Mock 数据与生产环境数据在内存布局上必须完全一致**。

**如果违反以下规定，程序将触发 `Access Violation (0xc0000005)` 导致整个测试进程崩溃。**

------

## 🛠 编写准则

### 1. 禁用自定义 Mock 消息结构体

- **禁止行为**：严禁在 Bench 脚本中定义私有的 `struct MockMessage` 或类似结构来模拟消息。
- **原因**：自定义结构体（Struct）与 Handler 预期的枚举类型（Enum）在内存中的对齐方式和字段偏移量不同。当宏强行将两者转换时，会导致 Handler 访问到非法的内存地址。

### 2. 必须使用 `abi::logic_import` 的真实类型

- **要求**：所有用于基准测试的 `Context` 必须使用项目中定义的真实 `Message` 或 `Notice` 枚举作为消息负载。
- **推荐做法**：在 `routing.rs` 或专门的辅助文件中编写基于真实枚举的构造函数：

Rust

```
// 正确示例：构造真实的 Message::Private 
fn create_real_context(text: &str) -> Context<MockClient, Message> {
    let msg = Message::Private(Private {
        time: 1700000000,
        raw_message: text.to_string(),
        message: MessageReceive::Single(SegmentReceive::Text(DataReceive { text: text.into() })),
        // ... 填充其他必要字段
    });
    Context::new(Arc::new(MockClient), Arc::new(msg))
}
```

### 3. 深度遍历兼容性

- **背景**：`archive` 模块（如 `message_archive`）会深度遍历消息的所有字段进行序列化和存储。
- **要求**：构造测试消息时，必须确保内部嵌套的对象（如 `Sender`、`MessageReceive`）不是空指针或随机值，必须符合反序列化后的真实状态。

### 4. 路由逻辑一致性

- **要求**：Mock 数据的 `get_type()` 返回值必须与被测 Handler 在 `#[handler(msg_type = ...)]` 中定义的类型严格匹配。
- **后果**：如果类型名匹配但内存布局不匹配，`type_filter` 会放行执行流，随后 `transmute` 逻辑会立即导致非法内存访问。

------

## 📝 给 LLM 的 Prompt 指令 (建议粘贴)

> "在为本项目编写或修改 Bench 脚本时，请遵循以下约束：
>
> 1. **不要** 定义任何自定义的消息结构体（如 `MockMessage`）。
> 2. **必须** 使用 `xmu_assistant_bot::abi::logic_import::Message` 或 `Notice` 枚举构造测试数据。
> 3. 确保 `Context` 的第二个泛型参数是真实的枚举类型，以匹配 `handler` 宏内部的 `unsafe transmute` 逻辑。
> 4. 填充所有必要的嵌套字段，以支持 `archive` 模块的深度序列化测试。"

