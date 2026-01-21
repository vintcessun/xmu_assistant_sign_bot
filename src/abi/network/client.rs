use anyhow::Result;
use async_trait::async_trait;
use serde::Serialize;
use std::fmt;

use crate::abi::{
    echo::Echo,
    message::{Params, api},
};

#[async_trait]
pub trait BotClient {
    async fn call_api<'a, T: Params + Serialize + fmt::Debug>(
        &'a self,
        params: &'a T,
        echo: Echo,
    ) -> Result<api::ApiResponsePending<T::Response>>;
}
