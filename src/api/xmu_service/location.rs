use std::{
    fmt::Debug,
    sync::{Arc, LazyLock},
};

pub static LOCATIONS: LazyLock<LocationService> = LazyLock::new(|| {
    LocationService::new([
        Location::new(
            Region::XiangAn,
            "学武",
            118.31378877355019,
            24.605488185517828,
        ),
        Location::new(
            Region::XiangAn,
            "西部片区2号",
            118.29990415225086,
            24.604252592181105,
        ),
        Location::new(
            Region::XiangAn,
            "西部片区4号",
            118.30018608783871,
            24.60527088060157,
        ),
        Location::new(
            Region::XiangAn,
            "文宣",
            118.30996445721189,
            24.60527978135845,
        ),
        Location::new(
            Region::XiangAn,
            "坤銮",
            118.31274420308284,
            24.605588434562396,
        ),
        Location::new(
            Region::XiangAn,
            "南存钿",
            118.31886473831305,
            24.604957055251823,
        ),
        Location::new(
            Region::XiangAn,
            "一期田径场",
            118.31887190174575,
            24.608956831402885,
        ),
        Location::new(
            Region::XiangAn,
            "佘明培游泳馆",
            118.31191862035234,
            24.610805475596482,
        ),
        Location::new(
            Region::XiangAn,
            "爱秋体育馆",
            118.31051001664491,
            24.61151956997547,
        ),
        Location::new(
            Region::XiangAn,
            "一期篮球场",
            118.31723671918621,
            24.608388221582917,
        ),
        Location::new(
            Region::XiangAn,
            "新工科",
            118.3103062983896,
            24.614680157143653,
        ),
        Location::new(
            Region::XiangAn,
            "德旺图书馆",
            118.3114093314407,
            24.60559975405282,
        ),
        Location::new(
            Region::XiangAn,
            "教学楼5号",
            118.30905177451268,
            24.604890125792558,
        ),
        Location::new(
            Region::XiangAn,
            "二期田径场",
            118.30266438251522,
            24.609405980616618,
        ),
        Location::new(
            Region::XiangAn,
            "二期篮球场",
            118.30300283434042,
            24.610474511279367,
        ),
        Location::new(
            Region::XiangAn,
            "西部片区正信",
            118.30038925732242,
            24.603583346376745,
        ),
        Location::new(
            Region::XiangAn,
            "西部片区益海嘉里",
            118.30099810060062,
            24.60453827134765,
        ),
        Location::new(
            Region::XiangAn,
            "西部片区5号",
            118.30151290244908,
            24.60557803258615,
        ),
        Location::new(
            Region::SiMing,
            "海韵",
            118.1138402432016,
            24.430413215400165,
        ),
        Location::new(
            Region::SiMing,
            "庄汉水",
            118.09651925805679,
            24.43778247340642,
        ),
    ])
});

#[derive(Clone)]
pub enum Region {
    XiangAn,
    SiMing,
}

impl Debug for Region {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Region::XiangAn => write!(f, "\"翔安校区\""),
            Region::SiMing => write!(f, "\"思明校区\""),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Location {
    pub region: Region,
    pub name: &'static str,
    pub longitude: f64,
    pub latitude: f64,
}

impl Location {
    pub fn new(region: Region, name: &'static str, longitude: f64, latitude: f64) -> Self {
        Self {
            region,
            name,
            longitude,
            latitude,
        }
    }
}

pub struct LocationService {
    pub locations: Vec<Arc<Location>>,
}

impl LocationService {
    pub fn new<const N: usize>(location: [Location; N]) -> Self {
        Self {
            locations: location.into_iter().map(Arc::new).collect(),
        }
    }

    pub fn query(&self, fullname: &str) -> Option<Location> {
        for loc in &self.locations {
            if fullname.contains(loc.name) {
                return Some(loc.as_ref().clone());
            }
        }
        None
    }
}
