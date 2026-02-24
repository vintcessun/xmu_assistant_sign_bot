use crate::{
    api::{
        network::SessionClient,
        xmu_service::{jw::LocationStore, lnt::StudentRollcalls, location::LOCATIONS},
    },
    logic::rollcall::data::{SIGN_LOCATION_DATA, SIGN_NUMBER_DATA},
};
use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use smol_str::SmolStr;
use std::sync::Arc;

#[derive(Serialize, Deserialize, Debug)]
pub struct RadarSign {
    pub distance: f64,
    //id: Option<Value>,
    //status: Option<Value>,
    //status_name: Option<Value>,
}

pub struct SignData;

impl SignData {
    pub async fn number(client: &SessionClient, activity_id: i64) -> Result<Arc<SmolStr>> {
        if let Some(number) = SIGN_NUMBER_DATA.get(&activity_id) {
            return Ok(number.clone());
        }
        let rollcall_data = StudentRollcalls::get_from_client(client, activity_id).await?;

        let number_data = rollcall_data.number_code.ok_or(anyhow!("无法获取签到码"))?;
        let number_data = Arc::new(SmolStr::new(number_data));
        SIGN_NUMBER_DATA.insert(activity_id, number_data.clone())?;
        Ok(number_data)
    }

    pub async fn location(activity_id: i64) -> Option<Arc<LocationStore>> {
        SIGN_LOCATION_DATA.get(&activity_id)
    }

    pub async fn location_write(activity_id: i64, location: Arc<LocationStore>) {
        SIGN_LOCATION_DATA.insert(activity_id, location).ok();
    }

    pub async fn location_retry(
        client: &SessionClient,
        activity_id: i64,
        device_id: &str,
    ) -> Result<Arc<LocationStore>> {
        if let Some(loc) = Self::location(activity_id).await {
            return Ok(loc);
        }

        let mut student_location = None;
        let mut student_distance = f64::MAX;

        for e in &*LOCATIONS {
            let lati = e.latitude;
            let long = e.longitude;

            let res = client
                .put(
                    format!(
                        "https://lnt.xmu.edu.cn/api/rollcall/{activity_id}/answer?api_version=1.1.2"
                    ),
                    &json!({
                        "deviceId": device_id,
                        "latitude": lati,
                        "longitude": long,
                        "speed": Value::Null,
                        "accuracy": 90,
                        "altitude": Value::Null,
                        "altitudeAccuracy": Value::Null,
                        "heading": Value::Null,
                    }),
                )
                .await?;

            let radar_data = res.json::<RadarSign>().await?;
            if radar_data.distance < student_distance {
                student_location = Some(e);
                student_distance = radar_data.distance;
            }

            if student_distance < 100.0 {
                break;
            };
        }

        if let Some(loc) = student_location
            && student_distance < 100.0
        {
            let loc: LocationStore = loc.to_owned().into();
            let loc = Arc::new(loc);
            Self::location_write(activity_id, loc.clone()).await;
            return Ok(loc);
        }
        bail!("无法获取有效的位置信息，最近的距离为 {student_distance} 米");
    }

    pub async fn location_remove(activity_id: i64) -> Result<()> {
        SIGN_LOCATION_DATA.remove(&activity_id)
    }
}
