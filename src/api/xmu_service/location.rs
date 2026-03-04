use serde::{Deserialize, Serialize};
use std::{
    fmt::Debug,
    sync::{Arc, LazyLock},
};

pub static LOCATIONS: LazyLock<LocationService> = LazyLock::new(|| {
    LocationService::new([
        Location::new(Region::XiangAn, "学武", 118.313_79, 24.605_488),
        Location::new(Region::XiangAn, "西部片区2号", 118.299_904, 24.604_252),
        Location::new(Region::XiangAn, "西部片区4号", 118.300_186, 24.605_27),
        Location::new(Region::XiangAn, "文宣", 118.309_97, 24.605_28),
        Location::new(Region::XiangAn, "坤銮", 118.312_744, 24.605_589),
        Location::new(Region::XiangAn, "南存钿", 118.318_86, 24.604_958),
        Location::new(Region::XiangAn, "一期田径场", 118.318_87, 24.608_957),
        Location::new(Region::XiangAn, "佘明培游泳馆", 118.311_92, 24.610_806),
        Location::new(Region::XiangAn, "爱秋体育馆", 118.310_51, 24.611_519),
        Location::new(Region::XiangAn, "一期篮球场", 118.317_24, 24.608_389),
        Location::new(Region::XiangAn, "新工科", 118.310_3, 24.614_68),
        Location::new(Region::XiangAn, "德旺图书馆", 118.311_41, 24.605_6),
        Location::new(Region::XiangAn, "教学楼5号", 118.309_05, 24.604_89),
        Location::new(Region::XiangAn, "二期田径场", 118.302_666, 24.609_406),
        Location::new(Region::XiangAn, "二期篮球场", 118.303, 24.610_474),
        Location::new(Region::XiangAn, "西部片区正信", 118.300_39, 24.603_584),
        Location::new(Region::XiangAn, "西部片区益海嘉里", 118.300_995, 24.604_538),
        Location::new(Region::XiangAn, "西部片区5号", 118.301_51, 24.605_577),
        Location::new(Region::SiMing, "海韵", 118.113_84, 24.430_412),
        Location::new(Region::SiMing, "庄汉水", 118.096_52, 24.437_782),
        Location::new(Region::XiangAn, "航院大楼", 118.311126, 24.60862),
    ])
});

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    pub region: Region,
    pub name: &'static str,
    pub longitude: f32,
    pub latitude: f32,
}

impl PartialEq for Location {
    fn eq(&self, other: &Self) -> bool {
        self.region == other.region && self.name == other.name
    }
}

impl Eq for Location {}

impl Location {
    pub fn new(region: Region, name: &'static str, longitude: f32, latitude: f32) -> Self {
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

impl IntoIterator for LocationService {
    type Item = Arc<Location>;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.locations.into_iter()
    }
}

impl<'a> IntoIterator for &'a LocationService {
    type Item = &'a Arc<Location>;
    type IntoIter = std::slice::Iter<'a, Arc<Location>>;

    fn into_iter(self) -> Self::IntoIter {
        self.locations.iter()
    }
}
