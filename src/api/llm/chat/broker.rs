/// LogicCommandBroker — 命令代理（Phase G + Phase H）
///
/// 功能：复用当前会话 Context，在执行命令前向 message_text 前插固定指令前缀，
/// 再通过标准命令分发路径执行，保持 message_list、状态与链路一致。
/// 命令零限制：无审批、无校验、无限流，所有 logic command 默认可执行。
use crate::{
    abi::{Context, logic_import::Message, network::BotClient, websocket::BotHandler},
    config::get_command_prefix,
    logic::{GENERATED_COMMANDS, dispatch_all_handlers},
};
use tracing::{info, warn};

/// 在 logic/mod.rs 中 build.rs 自动维护的 command 列表里提取的可用命令名。
/// 此列表随 build.rs 扫描结果自动同步（需在 build.rs 输出变化时更新）。
pub const AVAILABLE_COMMANDS: &[&str] = &[
    "help",
    "logout_pwd",
    "get_class",
    "login_pwd",
    "timetable",
    "download",
    "get_test",
    "test_ans",
    "github",
    "logout",
    "class",
    "image",
    "login",
    "echo",
    "fate",
    "test",
];

/// 判断给定的命令名是否在 Broker 可调用列表中（不含前缀）。
pub fn is_registered(command_name: &str) -> bool {
    AVAILABLE_COMMANDS.contains(&command_name)
}

/// 通过 Broker 执行一条命令。
///
/// - `command_name`：命令名，不含前缀（例如 `"timetable"`）。
/// - `args`：命令参数部分（可为空字符串）。
///
/// Broker 将 `message_text` 替换为 `{prefix}{command_name} {args}`，
/// 然后调用 `dispatch_all_handlers`，走与用户直接输入命令完全相同的路径。
pub fn dispatch<T>(ctx: &mut Context<T, Message>, command_name: &str, args: &str)
where
    T: BotClient + BotHandler + std::fmt::Debug + Send + Sync + 'static,
{
    let prefix = get_command_prefix();
    let injected = if args.is_empty() {
        format!("{}{}", prefix, command_name)
    } else {
        format!("{}{} {}", prefix, command_name, args)
    };

    info!(
        command = command_name,
        injected_text = %injected,
        "LogicCommandBroker: 前插指令前缀，准备分发命令"
    );

    ctx.set_message_text(injected.as_str());
    // 以当前 ctx 为基础克隆，保持 message_list 与会话状态一致
    dispatch_all_handlers(ctx.clone());
}

/// 启动期一致性检查（Phase H）：仅观测，不阻断执行。
///
/// 对比 [`AVAILABLE_COMMANDS`] 与 `build.rs` 自动生成的 `logic::GENERATED_COMMANDS`。
/// 发现差异时输出 warn 日志提示手工同步 broker.rs 常量。
///
/// 调用方：`main.rs` 启动初始化阶段。
pub fn check_consistency() {
    let mut missing_in_broker: Vec<&str> = Vec::new();
    let mut extra_in_broker: Vec<&str> = Vec::new();

    for cmd in GENERATED_COMMANDS {
        if !AVAILABLE_COMMANDS.contains(cmd) {
            missing_in_broker.push(cmd);
        }
    }
    for cmd in AVAILABLE_COMMANDS {
        if !GENERATED_COMMANDS.contains(cmd) {
            extra_in_broker.push(cmd);
        }
    }

    if missing_in_broker.is_empty() && extra_in_broker.is_empty() {
        info!("LogicCommandBroker 一致性检查通过：AVAILABLE_COMMANDS 与 logic/mod.rs 命令列表一致");
    } else {
        if !missing_in_broker.is_empty() {
            warn!(
                missing = ?missing_in_broker,
                "LogicCommandBroker 一致性警告：以下命令在 logic/mod.rs 中存在但 AVAILABLE_COMMANDS 缺失，请更新 broker.rs"
            );
        }
        if !extra_in_broker.is_empty() {
            warn!(
                extra = ?extra_in_broker,
                "LogicCommandBroker 一致性警告：以下命令在 AVAILABLE_COMMANDS 中存在但 logic/mod.rs 无对应命令，请核查是否已删除"
            );
        }
    }
}
