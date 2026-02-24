pub use crate::__hidden_lib_client_init as client_init;
pub use crate::__hidden_lib_get_client as get_client;

#[macro_export]
macro_rules! init_bot_global {
    ($t:ty) => {
        static BOT_CLIENT: tokio::sync::OnceCell<std::sync::Arc<$t>> =
            tokio::sync::OnceCell::const_new();

        pub fn __hidden_lib_get_client() -> &'static std::sync::Arc<$t> {
            BOT_CLIENT.get().expect("尚未初始化")
        }

        pub fn __hidden_lib_client_init(client: std::sync::Arc<$t>) {
            BOT_CLIENT.set(client).expect("重复初始化");
        }
    };
}
