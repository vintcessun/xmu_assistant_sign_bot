use crate::abi::message::api::{ApiResponse, Data};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct SpecialTitleData {}

impl Data for SpecialTitleData {}

pub type SpecialTitleResponse = ApiResponse<SpecialTitleData>;
