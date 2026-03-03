use crate::{
    api::{
        network::SessionClient,
        xmu_service::{
            jw::LocationStore,
            lnt::{StudentRollcalls, rollcalls::RollcallStatus, student_rollcalls::Status},
            location::LOCATIONS,
        },
    },
    logic::rollcall::data::{SIGN_LOCATION_DATA, SIGN_NUMBER_DATA},
};
use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use smol_str::SmolStr;
use std::fmt::Display;
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
                .put_json(
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

pub mod sign_response {
    use super::*;

    #[derive(Serialize, Deserialize, Debug)]
    pub struct RadarSign {}

    #[derive(Serialize, Deserialize, Debug)]
    pub struct NumberSign {
        pub number_code: Arc<SmolStr>,
    }

    #[derive(Serialize, Deserialize, Debug)]
    pub struct QRSign {}
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", content = "data")]
#[serde(rename_all = "snake_case")]
pub enum SignEnumResponse {
    Radar(sign_response::RadarSign),
    Number(sign_response::NumberSign),
    Qr(sign_response::QRSign),
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SignResponse {
    pub builder: SignResponseBuilder,
    pub extra: SignEnumResponse,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SignResponseBuilder {
    pub course_title: String,
    pub course_code: String,
    pub activity_id: i64,
    pub instructors: Vec<String>,
    pub sign_num: usize,
    pub student_num: usize,
    pub status: RollcallStatus,
}

impl SignResponseBuilder {
    pub fn radar(self) -> SignResponse {
        SignResponse {
            extra: SignEnumResponse::Radar(sign_response::RadarSign {}),
            builder: self,
        }
    }

    pub fn number(self, number_code: Arc<SmolStr>) -> SignResponse {
        SignResponse {
            extra: SignEnumResponse::Number(sign_response::NumberSign { number_code }),
            builder: self,
        }
    }

    pub fn qr(self) -> SignResponse {
        SignResponse {
            extra: SignEnumResponse::Qr(sign_response::QRSign {}),
            builder: self,
        }
    }
}

impl SignResponse {
    pub fn create(
        course_title: String,
        course_code: String,
        activity_id: i64,
        instructors: Vec<String>,
        sign_num: usize,
        student_num: usize,
        status: RollcallStatus,
    ) -> SignResponseBuilder {
        SignResponseBuilder {
            course_title,
            course_code,
            activity_id,
            instructors,
            sign_num,
            student_num,
            status,
        }
    }
}

pub async fn get_on_call_total_num(
    client: &SessionClient,
    rollcall_id: i64,
) -> Result<(usize, usize)> {
    let rollcall_response = StudentRollcalls::get_from_client(client, rollcall_id).await?;

    let on_call_num = rollcall_response
        .student_rollcalls
        .iter()
        .map(|x| match x.status {
            Status::OnCall => 1,
            Status::Absent => 0,
        })
        .sum::<_>();

    let total_num = rollcall_response.student_rollcalls.len();

    Ok((on_call_num, total_num))
}

impl Display for SignResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.extra {
            SignEnumResponse::Radar(_) => writeln!(f, "签到类型: 雷达签到"),
            SignEnumResponse::Number(data) => {
                writeln!(
                    f,
                    r#"签到类型: 数字签到
签到码: {}"#,
                    data.number_code
                )
            }
            SignEnumResponse::Qr(_) => writeln!(f, "签到类型: 二维码签到"),
        }?;
        let course_title = &self.builder.course_title;
        let course_code = &self.builder.course_code;
        let activity_id = self.builder.activity_id;
        let instructors = &self.builder.instructors;
        let sign_num = self.builder.sign_num;
        let student_num = self.builder.student_num;
        let status = &self.builder.status;
        write!(
            f,
            r#"课程：{course_title}({course_code})
签到ID：{activity_id}
教师：{instructors:?}
签到人数：{sign_num}/{student_num}
签到状态：{status}"#
        )?;
        Ok(())
    }
}
