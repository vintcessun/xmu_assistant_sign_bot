use serde::{Deserialize, Serialize};

pub mod auto_sign_response {
    use super::*;

    #[derive(Serialize, Deserialize, Debug)]
    #[serde(tag = "status", content = "data")]
    #[serde(rename_all = "snake_case")]
    pub enum RadarSign {
        Success(radar::Success),
        AlreadySigned(radar::AlreadySigned),
    }
    pub mod radar {
        use super::*;
        #[derive(Serialize, Deserialize, Debug)]
        pub struct Success {
            pub course_name: String,
            pub student_location: String,
            pub latitude: f64,
            pub longitude: f64,
            pub student_distance: f64,
        }

        #[derive(Serialize, Deserialize, Debug)]
        pub struct AlreadySigned {
            pub course_name: String,
        }
    }

    #[derive(Serialize, Deserialize, Debug)]
    #[serde(tag = "status", content = "data")]
    #[serde(rename_all = "snake_case")]
    pub enum NumberSign {
        Success(number::Success),
        AlreadySigned(number::AlreadySigned),
    }

    pub mod number {
        use super::*;
        #[derive(Serialize, Deserialize, Debug)]
        pub struct Success {
            pub course_name: String,
            pub number_code: String,
        }

        #[derive(Serialize, Deserialize, Debug)]
        pub struct AlreadySigned {
            pub course_name: String,
        }
    }

    #[derive(Serialize, Deserialize, Debug)]
    #[serde(tag = "status", content = "data")]
    #[serde(rename_all = "snake_case")]
    pub enum QRSign {
        Success(qr::Success),
        AlreadySigned(qr::AlreadySigned),
    }

    pub mod qr {
        use super::*;
        #[derive(Serialize, Deserialize, Debug)]
        pub struct Success {
            pub course_name: String,
        }

        #[derive(Serialize, Deserialize, Debug)]
        pub struct AlreadySigned {
            pub course_name: String,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", content = "data")]
#[serde(rename_all = "snake_case")]
pub enum AutoSignData {
    Radar(auto_sign_response::RadarSign),
    Number(auto_sign_response::NumberSign),
    Qr(auto_sign_response::QRSign),
}

impl AutoSignData {
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

    pub fn qr_success(course_name: String) -> Self {
        Self::Qr(auto_sign_response::QRSign::Success(
            auto_sign_response::qr::Success { course_name },
        ))
    }

    pub fn qr_already_signed(course_name: String) -> Self {
        Self::Qr(auto_sign_response::QRSign::AlreadySigned(
            auto_sign_response::qr::AlreadySigned { course_name },
        ))
    }
}

impl AutoSignData {}
