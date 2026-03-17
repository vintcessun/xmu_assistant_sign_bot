use serde::{Deserialize, Serialize};
use std::fmt::Display;

use crate::logic::rollcall::auto_sign_data::auto_sign_response::qr::QRSignSuccessResult;

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
            pub latitude: f32,
            pub longitude: f32,
            pub student_distance: f64,
            pub try_type: RadarType,
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
            pub sign_result: QRSignSuccessResult,
        }

        impl Display for QRSignSuccessResult {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    QRSignSuccessResult::Success(data) => match &data.status {
                        QRSignOkStatus::OnCall => {
                            write!(f, "签到成功，签到id为: {}", data.id)
                        }
                    },
                    QRSignSuccessResult::Failed(data) => match &data.error_code {
                        QRSignErrCode::RollcallAlreadyClosed => {
                            write!(f, "签到已结束")
                        }
                        QRSignErrCode::QRCodeExpired => {
                            write!(f, "二维码已过期")
                        }
                    },
                }
            }
        }

        #[cfg(test)]
        mod tests {
            #[test]
            pub fn parse_ok() {
                let json = r#"{"id":16686807,"status":"on_call"}"#;
                let res: super::QRSignSuccessResult = serde_json::from_str(json).unwrap();
                println!("解析结果: {:?}", res);
            }

            #[test]
            pub fn parse_err_expired() {
                let json = r#"{"error_code":"qr_code_expired","message":"QR_code_expired"}"#;
                let res: super::QRSignSuccessResult = serde_json::from_str(json).unwrap();
                println!("解析结果: {:?}", res);
            }

            #[test]
            pub fn parse_err_closed() {
                let json =
                    r#"{"error_code":"rollcall_already_closed","message":"rollcall_closed"}"#;
                let res: super::QRSignSuccessResult = serde_json::from_str(json).unwrap();
                println!("解析结果: {:?}", res);
            }
        }

        #[derive(Serialize, Deserialize, Debug, Clone)]
        #[serde(untagged)]
        pub enum QRSignSuccessResult {
            Success(QRSignOk),
            Failed(QRSignErr),
        }

        #[derive(Serialize, Deserialize, Debug, Clone)]
        pub struct QRSignOk {
            pub id: i64,
            pub status: QRSignOkStatus,
        }

        #[derive(Serialize, Deserialize, Debug, Clone)]
        #[serde(rename_all = "snake_case")]
        pub enum QRSignOkStatus {
            OnCall,
        }

        #[derive(Serialize, Deserialize, Debug, Clone)]
        pub struct QRSignErr {
            pub error_code: QRSignErrCode,
            pub message: QRSignErrMsg,
        }

        #[derive(Serialize, Deserialize, Debug, Clone)]
        pub enum QRSignErrCode {
            #[serde(rename = "rollcall_already_closed")]
            RollcallAlreadyClosed,
            #[serde(rename = "qr_code_expired")]
            QRCodeExpired,
        }

        #[derive(Serialize, Deserialize, Debug, Clone)]
        pub enum QRSignErrMsg {
            #[serde(rename = "rollcall_closed")]
            RollcallAlreadyClosed,
            #[serde(rename = "QR_code_expired")]
            QRCodeExpired,
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
                        "成功雷达签到{}，签到位置为：{}({:.6}, {:.6})，距离为：{:.2}米，签到方法为: {}",
                        data.course_name,
                        data.student_location,
                        data.latitude,
                        data.longitude,
                        data.student_distance,
                        data.try_type
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy, Serialize, Deserialize)]
pub enum RadarType {
    Retry,
    Timetable,
    Cache,
    Triple,
}

impl Display for RadarType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Retry => write!(f, "多次尝试法"),
            Self::Timetable => write!(f, "课程表法"),
            Self::Cache => write!(f, "缓存法"),
            Self::Triple => write!(f, "三点定位法"),
        }?;
        Ok(())
    }
}

impl AutoSignResponse {
    pub fn radar_success(
        course_name: String,
        student_location: String,
        latitude: f32,
        longitude: f32,
        student_distance: f64,
        try_type: RadarType,
    ) -> Self {
        Self::Radar(auto_sign_response::RadarSign::Success(
            auto_sign_response::radar::Success {
                course_name,
                student_location,
                latitude,
                longitude,
                student_distance,
                try_type,
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

    pub fn qr_success(course_name: String, sign_result: QRSignSuccessResult) -> Self {
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
