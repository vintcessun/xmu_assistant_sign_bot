use super::data::LOGIN_DATA;
use crate::{
    api::{
        network::SessionClient,
        xmu_service::{
            jw::LocationStore,
            lnt::{CourseData, Profile},
        },
    },
    logic::rollcall::{
        data::TIMETABLE_DATA,
        sign_data::{RadarSign, SignData},
        utils::{generate_uuid, get_ts, string_similarity, uniform},
    },
};
use anyhow::{Result, anyhow};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fmt::Display;
use std::sync::Arc;
use std::sync::LazyLock;

pub mod auto_sign_response {
    use super::*;

    #[derive(Serialize, Deserialize, Debug, Clone)]
    #[serde(tag = "status", content = "data")]
    #[serde(rename_all = "snake_case")]
    pub enum RadarSign {
        Success(radar::Success),
        AlreadySigned(radar::AlreadySigned),
    }
    pub mod radar {
        use super::*;
        #[derive(Serialize, Deserialize, Debug, Clone)]
        pub struct Success {
            pub course_name: String,
            pub student_location: String,
            pub latitude: f64,
            pub longitude: f64,
            pub student_distance: f64,
        }

        #[derive(Serialize, Deserialize, Debug, Clone)]
        pub struct AlreadySigned {
            pub course_name: String,
        }
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    #[serde(tag = "status", content = "data")]
    #[serde(rename_all = "snake_case")]
    pub enum NumberSign {
        Success(number::Success),
        AlreadySigned(number::AlreadySigned),
    }

    pub mod number {
        use super::*;
        #[derive(Serialize, Deserialize, Debug, Clone)]
        pub struct Success {
            pub course_name: String,
            pub number_code: String,
        }

        #[derive(Serialize, Deserialize, Debug, Clone)]
        pub struct AlreadySigned {
            pub course_name: String,
        }
    }

    #[derive(Serialize, Deserialize, Debug, Clone)]
    #[serde(tag = "status", content = "data")]
    #[serde(rename_all = "snake_case")]
    pub enum QRSign {
        Success(qr::Success),
        Pending(qr::Pending),
        AlreadySigned(qr::AlreadySigned),
    }

    pub mod qr {
        use super::*;
        #[derive(Serialize, Deserialize, Debug, Clone)]
        pub struct Success {
            pub course_name: String,
            pub sign_result: String,
        }

        #[derive(Serialize, Deserialize, Debug, Clone)]
        pub struct Pending {
            pub course_name: String,
        }

        #[derive(Serialize, Deserialize, Debug, Clone)]
        pub struct AlreadySigned {
            pub course_name: String,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type", content = "data")]
#[serde(rename_all = "snake_case")]
pub enum AutoSignResponse {
    Radar(auto_sign_response::RadarSign),
    Number(auto_sign_response::NumberSign),
    Qr(auto_sign_response::QRSign),
}

impl Display for AutoSignResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AutoSignResponse::Radar(radar_sign) => match radar_sign {
                auto_sign_response::RadarSign::Success(data) => {
                    write!(
                        f,
                        "成功雷达签到{}，签到位置为：{}({:.6}, {:.6})，距离为：{:.2}米",
                        data.course_name,
                        data.student_location,
                        data.latitude,
                        data.longitude,
                        data.student_distance
                    )?;
                }
                auto_sign_response::RadarSign::AlreadySigned(data) => {
                    write!(f, "雷达签到{}已签到", data.course_name)?;
                }
            },
            AutoSignResponse::Number(number_sign) => match number_sign {
                auto_sign_response::NumberSign::Success(data) => {
                    write!(
                        f,
                        "成功数字签到{}，签到码为{}",
                        data.course_name, data.number_code
                    )?;
                }
                auto_sign_response::NumberSign::AlreadySigned(data) => {
                    write!(f, "数字签到{}已签到", data.course_name)?;
                }
            },
            AutoSignResponse::Qr(qr_sign) => match qr_sign {
                auto_sign_response::QRSign::Success(data) => {
                    write!(
                        f,
                        "二维码签到成功{}，签到详情{}",
                        data.course_name, data.sign_result
                    )?;
                }
                auto_sign_response::QRSign::AlreadySigned(data) => {
                    write!(f, "二维码签到{}已签到", data.course_name)?;
                }
                auto_sign_response::QRSign::Pending(data) => {
                    write!(
                        f,
                        "未二维码签到{}，请用/sign查看状态，如果有人发送二维码会自动推送",
                        data.course_name
                    )?;
                }
            },
        }
        Ok(())
    }
}

impl AutoSignResponse {
    pub fn radar_success(
        course_name: String,
        student_location: String,
        latitude: f64,
        longitude: f64,
        student_distance: f64,
    ) -> Self {
        Self::Radar(auto_sign_response::RadarSign::Success(
            auto_sign_response::radar::Success {
                course_name,
                student_location,
                latitude,
                longitude,
                student_distance,
            },
        ))
    }

    pub fn radar_already_signed(course_name: String) -> Self {
        Self::Radar(auto_sign_response::RadarSign::AlreadySigned(
            auto_sign_response::radar::AlreadySigned { course_name },
        ))
    }

    pub fn number_success(course_name: String, number_code: String) -> Self {
        Self::Number(auto_sign_response::NumberSign::Success(
            auto_sign_response::number::Success {
                course_name,
                number_code,
            },
        ))
    }

    pub fn number_already_signed(course_name: String) -> Self {
        Self::Number(auto_sign_response::NumberSign::AlreadySigned(
            auto_sign_response::number::AlreadySigned { course_name },
        ))
    }

    pub fn qr_success(course_name: String, sign_result: String) -> Self {
        Self::Qr(auto_sign_response::QRSign::Success(
            auto_sign_response::qr::Success {
                course_name,
                sign_result,
            },
        ))
    }

    pub fn qr_already_signed(course_name: String) -> Self {
        Self::Qr(auto_sign_response::QRSign::AlreadySigned(
            auto_sign_response::qr::AlreadySigned { course_name },
        ))
    }

    pub fn qr_pending(course_name: String) -> Self {
        Self::Qr(auto_sign_response::QRSign::Pending(
            auto_sign_response::qr::Pending { course_name },
        ))
    }
}

pub struct AutoSignRequest {
    pub device_id: String,
    pub session: String,
    pub client: Arc<SessionClient>,
    pub course_id: i64,
    pub qq: i64,
    pub ua: &'static str,
}

impl AutoSignRequest {
    pub async fn number(&self, activity_id: i64) -> Result<AutoSignResponse> {
        let number = SignData::number(&self.client, activity_id).await?;
        let course_info = CourseData::get(&self.session, self.course_id).await?;
        let user_info = Profile::get(&self.session).await?;

        self.client
            .put_json(
                format!("https://lnt.xmu.edu.cn/api/rollcall/{activity_id}/answer_number_rollcall"),
                &json!({"deviceId": self.device_id, "numberCode": number}),
            )
            .await?;

        self.client
            .post_json(
                "https://lnt.xmu.edu.cn/statistics/api/learning-activity",
                &json!({
                    "is_mobile": true,
                    "user_agent": self.ua,
                    "user_id": user_info.id,
                    "user_no": user_info.user_no,
                    "user_name": user_info.name,
                    "org_id": 1,
                    "org_name": "课程中心",
                    "dep_name": user_info.department.name,
                    "course_id": self.course_id,
                    "activity_id": activity_id,
                    "activity_type": "rollcall",
                    "is_teacher": false,
                    "mode": "normal",
                    "enrollment_role": "student",
                    "channel": "app",
                    "ts": get_ts(),
                    "action": "sign",
                    "sub_type": "number",
                }),
            )
            .await?;

        Ok(AutoSignResponse::number_success(
            course_info.name.clone(),
            number.to_string(),
        ))
    }

    pub async fn radar(
        &self,
        activity_id: i64,
        loc_src: Arc<LocationStore>,
    ) -> Result<AutoSignResponse> {
        let loc = loc_src
            .pos
            .clone()
            .ok_or(anyhow!("位置设置错误，这个错误不应该发生"))?;
        let course_info = CourseData::get(&self.session, self.course_id).await?;
        let user_info = Profile::get(&self.session).await?;

        let lati = loc.latitude + uniform(-0.0003..0.0003);
        let long = loc.longitude + uniform(-0.0003..0.0003);

        let res = self
            .client
            .put_json(
                format!(
                    "https://lnt.xmu.edu.cn/api/rollcall/{activity_id}/answer?api_version=1.1.2",
                ),
                &json!({"deviceId": self.device_id,
                        "latitude": lati,
                        "longitude": long,
                        "speed": Value::Null,
                        "accuracy": 90,
                        "altitude": Value::Null,
                        "altitudeAccuracy": Value::Null,
                        "heading": Value::Null}),
            )
            .await?;

        let radar_data = res.json::<RadarSign>().await?;
        let student_distance = radar_data.distance;

        self.client
            .post_json(
                "https://lnt.xmu.edu.cn/statistics/api/learning-activity",
                &json!({
                    "is_mobile": true,
                        "user_agent": self.ua,
                        "user_id": user_info.id,
                        "user_no": user_info.user_no,
                        "user_name": user_info.name,
                        "org_id": 1,
                        "org_name": "课程中心",
                        "dep_name": user_info.department.name,
                        "course_id": self.course_id,
                        "activity_id": activity_id,
                        "activity_type": "rollcall",
                        "is_teacher": false,
                        "mode": "normal",
                        "enrollment_role": "student",
                        "channel": "app",
                        "ts": get_ts(),
                        "action": "sign",
                        "sub_type": "radar",}),
            )
            .await?;

        self.client
            .post_json(
                "https://lnt.xmu.edu.cn/statistics/api/rollcall/extra-data",
                &json!({
                        "is_mobile": true,
                        "user_agent": self.ua,
                        "user_id": user_info.id,
                        "user_no": user_info.user_no,
                        "user_name": user_info.name,
                        "org_id": 1,
                        "org_name": "课程中心",
                        "dep_id": user_info.department.id,
                        "dep_name": user_info.department.name,
                        "course_id": self.course_id,
                        "course_code": course_info.course_code,
                        "course_name": course_info.name,
                        "is_teacher": false,
                        "action_type": "participate",
                        "ts": get_ts(),
                        "rollcall_id": activity_id,
                        "accuracy": 90,
                        "student_distance": student_distance,
                        "device_id": self.device_id,
                        "student_status": "on_call_fine",
                        "location": {"lat": lati, "lon": long},}),
            )
            .await?;

        if student_distance < 100.0 {
            SignData::location_write(activity_id, loc_src).await;
        } else if student_distance > 300.0 {
            SignData::location_remove(activity_id).await?;
        }

        Ok(AutoSignResponse::radar_success(
            course_info.name.clone(),
            loc.name.to_string(),
            loc.latitude,
            loc.longitude,
            student_distance,
        ))
    }

    pub async fn radar_retry(&self, activity_id: i64) -> Result<AutoSignResponse> {
        let loc = SignData::location_retry(&self.client, activity_id, &self.device_id).await?;
        self.radar(activity_id, loc).await
    }

    pub async fn qr(&self, activity_id: i64, qrcode: &str) -> Result<AutoSignResponse> {
        let course_info = CourseData::get(&self.session, self.course_id).await?;
        let user_info = Profile::get(&self.session).await?;

        let res = self
            .client
            .put_json(
                format!("https://lnt.xmu.edu.cn/api/rollcall/{activity_id}/answer_qr_rollcall"),
                &json!({
                    "data": qrcode,
                    "deviceId": self.device_id,
                }),
            )
            .await?;

        self.client
            .post_json(
                "https://lnt.xmu.edu.cn/statistics/api/learning-activity",
                &json!({
                    "is_mobile": true,
                    "user_agent": self.ua,
                    "user_id": user_info.id,
                    "user_no": user_info.user_no,
                    "user_name": user_info.name,
                    "org_id": 1,
                    "org_name": "课程中心",
                    "dep_name": user_info.department.name,
                    "course_id": self.course_id,
                    "activity_id": activity_id,
                    "activity_type": "rollcall",
                    "is_teacher": false,
                    "mode": "normal",
                    "enrollment_role": "student",
                    "channel": "app",
                    "ts": get_ts(),
                    "action": "sign",
                    "sub_type": "qr",
                }),
            )
            .await?;

        let sign_result = res.text().await?.replace(['\n', ' '], "");

        Ok(AutoSignResponse::qr_success(
            course_info.name.clone(),
            sign_result,
        ))
    }

    pub async fn radar_timetable(&self, activity_id: i64) -> Result<AutoSignResponse> {
        let data = TIMETABLE_DATA
            .get(&self.qq)
            .ok_or(anyhow!("未找到用户的课程表数据"))?;
        let course_info = CourseData::get(&self.session, self.course_id).await?;

        let similarity_cal =
            |course_name: &str| -> f64 { string_similarity(course_name, &course_info.name) };

        let loc = data
            .times
            .iter()
            .map(|x| (similarity_cal(&x.name), x))
            .max_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

        let loc = loc.ok_or(anyhow!("获取时间表位置失败"))?.1.location.clone();

        self.radar(activity_id, loc).await
    }
}

static AUTO_SIGN_CACHE: LazyLock<DashMap<(i64, i64), Arc<AutoSignRequest>>> =
    LazyLock::new(DashMap::new);

impl AutoSignRequest {
    pub async fn get(course_id: i64, qq: i64, client: Arc<SessionClient>) -> Result<Arc<Self>> {
        if let Some(entry) = AUTO_SIGN_CACHE.get(&(qq, course_id)) {
            Ok(entry.value().clone())
        } else {
            let session = LOGIN_DATA.get(&qq).ok_or(anyhow!("请先登录"))?;
            let ua = client.get_ua();
            let ret = Arc::new(Self {
                device_id: generate_uuid(),
                session: session.lnt.clone(),
                client,
                course_id,
                qq,
                ua,
            });
            AUTO_SIGN_CACHE.insert((qq, course_id), ret.clone());
            Ok(ret)
        }
    }
}
