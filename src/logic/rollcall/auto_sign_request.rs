use super::data::LOGIN_DATA;
use crate::{
    abi::utils::SmartJsonExt,
    api::{
        network::SessionClient,
        xmu_service::{
            jw::LocationStore,
            lnt::{CourseData, Profile},
        },
    },
    logic::rollcall::{
        auto_sign_data::{
            AutoSignResponse, RadarType, auto_sign_response::qr::QRSignSuccessResult,
        },
        data::TIMETABLE_DATA,
        sign_data::{RadarSign, SignData},
        utils::{generate_uuid, get_ts, string_similarity, uniform},
    },
};
use anyhow::{Result, anyhow, bail};
use dashmap::DashMap;
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::LazyLock;
use tracing::trace;

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
}

impl AutoSignRequest {
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

        let sign_result = res.json_smart::<QRSignSuccessResult>().await?;

        Ok(AutoSignResponse::qr_success(
            course_info.name.clone(),
            sign_result,
        ))
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

impl AutoSignRequest {
    pub async fn radar_distance(
        client: &SessionClient,
        device_id: &str,
        activity_id: i64,
        latitude: f32,
        longitude: f32,
    ) -> Result<f64> {
        let accuracy = uniform(80.0..120.0);
        let res = client
            .put_json(
                format!(
                    "https://lnt.xmu.edu.cn/api/rollcall/{activity_id}/answer?api_version=1.76",
                ),
                &json!({"deviceId": device_id,
                        "latitude": latitude,
                        "longitude": longitude,
                        "speed": Value::Null,
                        "accuracy": accuracy,
                        "altitude": Value::Null,
                        "altitudeAccuracy": Value::Null,
                        "heading": Value::Null}),
            )
            .await?;

        let radar_data = res.json::<RadarSign>().await?;
        Ok(radar_data.distance)
    }

    async fn radar_inner(
        &self,
        activity_id: i64,
        loc_src: Arc<LocationStore>,
        random: bool,
        try_type: RadarType,
    ) -> Result<AutoSignResponse> {
        let loc = loc_src
            .pos
            .clone()
            .ok_or(anyhow!("位置设置错误，这个错误不应该发生"))?;
        let course_info = CourseData::get(&self.session, self.course_id).await?;
        let user_info = Profile::get(&self.session).await?;

        let (latitude, longitude) = if random {
            (
                loc.latitude + uniform(-0.0003..0.0003),
                loc.longitude + uniform(-0.0003..0.0003),
            )
        } else {
            (loc.latitude, loc.longitude)
        };

        let student_distance = Self::radar_distance(
            &self.client,
            &self.device_id,
            activity_id,
            latitude,
            longitude,
        )
        .await?;

        if student_distance > 300.0 {
            trace!(distance = student_distance, "签到距离较远，可能签到失败");
            bail!("签到失败，距离过远，距离: {student_distance} 米");
        }

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
                        "location": {"lat": latitude, "lon": longitude},}),
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
            try_type,
        ))
    }

    async fn radar_retry(&self, activity_id: i64) -> Result<AutoSignResponse> {
        let (loc, try_type) =
            SignData::location_retry(&self.client, activity_id, &self.device_id).await?;
        self.radar_inner(activity_id, loc, true, try_type).await
    }

    async fn radar_timetable(&self, activity_id: i64) -> Result<AutoSignResponse> {
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

        self.radar_inner(activity_id, loc, true, RadarType::Timetable)
            .await
    }

    async fn radar_triple(&self, activity_id: i64) -> Result<AutoSignResponse> {
        let loc = SignData::location_fix_triple(&self.client, activity_id, &self.device_id).await?;

        self.radar_inner(activity_id, loc, true, RadarType::Triple)
            .await
    }
}

impl AutoSignRequest {
    pub async fn radar(&self, activity_id: i64) -> Result<AutoSignResponse> {
        if let Ok(loc) = self.radar_timetable(activity_id).await {
            return Ok(loc);
        }
        if let Ok(loc) = self.radar_triple(activity_id).await {
            return Ok(loc);
        }
        if let Ok(loc) = self.radar_retry(activity_id).await {
            return Ok(loc);
        }
        bail!("所有尝试雷达签到失败")
    }
}
