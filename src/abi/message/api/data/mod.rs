mod get_forward_msg;
mod get_group_info;
mod group_member_info;
mod poke;
mod send_msg;
mod title;

pub use get_forward_msg::*;
pub use get_group_info::*;
pub use group_member_info::*;
pub use poke::*;
pub use send_msg::*;
pub use title::*;

use crate::abi::echo::{Echo, EchoPending};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;

pub trait Data: Send + Sync + 'static + Serialize {}

pub struct ApiResponsePending<R> {
    pub echo: EchoPending,
    _marker: PhantomData<R>,
}

impl<R: ApiResponseTrait + for<'de> Deserialize<'de>> ApiResponsePending<R> {
    pub fn new(echo: Echo) -> Self {
        Self {
            echo: EchoPending::new(echo),
            _marker: PhantomData,
        }
    }

    pub async fn wait_echo(self) -> Result<R> {
        let response_bytes = self.echo.wait().await?;
        let response = serde_json::from_slice::<R>(response_bytes.as_bytes())?;
        Ok(response)
    }
}

pub trait ApiResponseTrait {}

#[derive(Serialize, Deserialize, Debug)]
pub struct ApiResponse<T: Data> {
    pub status: Status,
    pub retcode: u16,
    pub message: Option<String>,
    pub data: Option<T>,
    pub echo: Echo,
    pub wording: Option<String>,
    pub stream: Option<Stream>,
}

impl<T: Data> ApiResponseTrait for ApiResponse<T> {}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
pub enum Stream {
    StreamAction,
    NormalAction,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    Async,
    Failed,
}
