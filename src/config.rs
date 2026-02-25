use std::fs;

use serde::Serialize;
use tracing::{error, info};

const CONFIG: Config = Config {
    napcat: ServerConfig {
        host: "127.0.0.1",
        port: 3008,
        access_token: None,
        reconnect_interval_secs: 10,
    },
    bot: BotConfig {
        command_prefix: "/",
        self_qq: "3451169043",
    },
};

pub fn ensure_dir(path: &'static str) -> &'static str {
    if let Err(e) = fs::create_dir_all(path) {
        error!(path = ?path, error = ?e, "创建必要的目录失败，程序将退出");
        panic!("创建必要的目录失败");
    }
    info!(path = ?path, "已确保目录存在");
    path
}

pub fn get_self_qq() -> &'static str {
    CONFIG.bot.self_qq
}

pub const DATA_DIR: &str = "./data";

pub const fn get_command_prefix() -> &'static str {
    CONFIG.bot.command_prefix
}

pub const fn get_napcat_config() -> ServerConfig {
    CONFIG.napcat
}

#[derive(Serialize, Debug, Default, Clone)]
pub struct Config {
    pub napcat: ServerConfig,
    pub bot: BotConfig,
}

#[derive(Serialize, Debug, Clone)]
pub struct ServerConfig {
    pub host: &'static str,
    pub port: u16,
    pub access_token: Option<&'static str>,
    pub reconnect_interval_secs: u64,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            host: "127.0.0.1",
            port: 3001,
            access_token: None,
            reconnect_interval_secs: 10,
        }
    }
}

#[derive(Serialize, Debug, Clone)]
pub struct BotConfig {
    pub command_prefix: &'static str,
    pub self_qq: &'static str,
}

impl Default for BotConfig {
    fn default() -> Self {
        BotConfig {
            command_prefix: "/",
            self_qq: "3451169043",
        }
    }
}

pub const LLM_AUDIT_DURATION_SECS: u64 = 120;
