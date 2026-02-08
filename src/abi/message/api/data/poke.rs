use crate::abi::message::api::{ApiResponse, Data};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct PokeData {}

impl Data for PokeData {}

pub type PokeResponse = ApiResponse<PokeData>;
