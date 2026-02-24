pub mod abi;
pub mod api;
pub mod config;
pub mod logger;
pub mod logic;
pub mod web;

//声明全局 Adapter 类型
init_bot_global!(abi::network::NapcatAdapter);
