use crate::{
    api::{
        network::SessionClient,
        xmu_service::{
            jw::LocationStore,
            lnt::{StudentRollcalls, rollcalls::RollcallStatus, student_rollcalls::Status},
            location::LOCATIONS,
        },
    },
    logic::rollcall::{
        auto_sign_data::RadarType,
        auto_sign_request::AutoSignRequest,
        data::{SIGN_LOCATION_DATA, SIGN_NUMBER_DATA},
        location_utils::{GeoPoint, location_trilaterate},
    },
};
use anyhow::{Result, anyhow, bail};
use serde::{Deserialize, Serialize};
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
    ) -> Result<(Arc<LocationStore>, RadarType)> {
        if let Some(loc) = Self::location(activity_id).await {
            return Ok((loc, RadarType::Cache));
        }

        let mut student_location = None;
        let mut student_distance = f64::MAX;

        for e in &*LOCATIONS {
            let lati = e.latitude;
            let long = e.longitude;

            let radar_distance =
                AutoSignRequest::radar_distance(client, device_id, activity_id, lati, long).await?;
            if radar_distance < student_distance {
                student_location = Some(e);
                student_distance = radar_distance;
            }

            if student_distance < 100.0 {
                break;
            };
        }

        if let Some(loc) = student_location {
            let loc: LocationStore = loc.to_owned().into();
            let loc = Arc::new(loc);
            if student_distance < 100.0 {
                Self::location_write(activity_id, loc.clone()).await;
            }
            if student_distance < 200.0 {
                return Ok((loc, RadarType::Retry));
            }
        }
        bail!("无法获取有效的位置信息，最近的距离为 {student_distance} 米");
    }

    pub async fn location_fix_triple(
        client: &SessionClient,
        activity_id: i64,
        device_id: &str,
    ) -> Result<Arc<LocationStore>> {
        let mut location = Vec::with_capacity(3);
        for i in 0..3 {
            if let Some(loc) = LOCATIONS.get(i) {
                let radar_distance = AutoSignRequest::radar_distance(
                    client,
                    device_id,
                    activity_id,
                    loc.latitude,
                    loc.longitude,
                )
                .await?;
                location.push((loc, radar_distance));
            }
        }
        if location.len() < 3 {
            bail!("可用的位置信息不足，无法使用三次定位计算");
        }

        let ret = location_trilaterate(
            GeoPoint {
                lat: location[0].0.latitude,
                lon: location[0].0.longitude,
                dist: location[0].1,
            },
            GeoPoint {
                lat: location[1].0.latitude,
                lon: location[1].0.longitude,
                dist: location[1].1,
            },
            GeoPoint {
                lat: location[2].0.latitude,
                lon: location[2].0.longitude,
                dist: location[2].1,
            },
        );
        if let Some(loc) = ret
            && let Some(loc) = LOCATIONS.find(loc.lat, loc.lon, 100.0)
        {
            let sign_dis = AutoSignRequest::radar_distance(
                client,
                device_id,
                activity_id,
                loc.latitude,
                loc.longitude,
            )
            .await?;
            let loc: LocationStore = loc.to_owned().into();
            let loc = Arc::new(loc);
            if sign_dis < 100.0 {
                return Ok(loc);
            }
        }
        bail!("无法获取有效的位置信息");
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
