use anyhow::Result;
use chrono::{Datelike, Timelike, Utc};
use helper::tool;
use tokio::task::block_in_place;
use tracing::trace;

#[tool(description = r#"获取当前的时间、农历、二十四节气、星座等时间相关的信息"#)]
pub async fn time_info() -> Result<String> {
    let ret = time_info_getter().await?;
    trace!(time_info=?ret, "获取时间信息成功");
    Ok(ret)
}

pub async fn time_info_getter() -> Result<String> {
    block_in_place(time_info_getter_inner)
}

fn time_info_getter_inner() -> Result<String> {
    let mut ret = String::new();

    // 农历信息
    {
        use lunar_rust::{
            lunar::LunarRefHelper,
            solar::{self, SolarRefHelper},
        };
        let now = chrono::Local::now();
        let year = now.year();
        let month = now.month();
        let day = now.day();
        let hour = now.hour();
        let minute = now.minute();
        let second = now.second();

        let solar = solar::from_ymdhms(
            year as i64,
            month as i64,
            day as i64,
            hour as i64,
            minute as i64,
            second as i64,
        );
        let lunar = solar.get_lunar();

        //一九五七年九月初十 丁酉(鸡)年 庚戌(狗)月 丁丑(牛)日 戌(狗)时 纳音[山下火 钗钏金 涧下水 钗钏金] 星期五 西方白虎 星宿[娄金狗](吉) 彭祖百忌[丁不剃头头必生疮 丑不冠带主不还乡] 喜神方位[离](正南) 阳贵神方位[乾](西北) 阴贵神方位[兑](正西) 福神方 位[巽](东南) 财神方位[坤](西南) 冲[(辛未)羊] 煞[东]
        ret.push_str(&lunar.to_full_string());
        ret.push('\n');

        //1957-11-01 19:20:00 星期五 (万圣节) 天蝎座
        ret.push_str(&solar.to_full_string());
        ret.push('\n');
    }

    //星座信息
    {
        use astro::*;
        // --- 1. 指定时间并计算儒略日 ---
        let now = Utc::now();
        let jd = time::julian_day(&time::Date {
            year: now.year() as i16,
            month: now.month() as u8,
            decimal_day: now.day() as f64
                + (now.hour() as f64 / 24.0)
                + (now.minute() as f64 / 1440.0),
            cal_type: time::CalType::Gregorian,
        });
        // 计算 ΔT 并获取儒略历元 (Julian Ephemeris Day)
        let delta_t = time::delta_t(now.year(), now.month() as u8); // 修正：month 需转 i16
        let julian_ephm_day = time::julian_ephemeris_day(jd, delta_t);
        ret.push_str(&format!("当前儒略日: {:.5}\n", jd));

        // --- 2. 获取太阳和月亮位置 ---
        let (sun_ecl_point, _rad_vec_sun) = sun::geocent_ecl_pos(julian_ephm_day);
        let zodiac_sign = get_zodiac_sign(sun_ecl_point.long.to_degrees());
        ret.push_str(&format!("太阳所在星座: {}\n", zodiac_sign));

        //let (moon_ecl_point, _rad_vec_moon) = lunar::geocent_ecl_pos(julian_ephm_day);

        // --- 3. 获取行星相对于太阳的位置 ---
        //let (jup_long, _jup_lat, _rad_vec) =
        //    planet::heliocent_coords(&planet::Planet::Jupiter, julian_ephm_day);

        // --- 4. 计算地球两点间的距离 ---
        let paris = coords::GeographPoint {
            long: angle::deg_frm_dms(-2, 20, 14.0).to_radians(),
            lat: angle::deg_frm_dms(48, 50, 11.0).to_radians(),
        };

        let washington = coords::GeographPoint {
            long: angle::deg_frm_dms(77, 3, 56.0).to_radians(),
            lat: angle::deg_frm_dms(38, 55, 17.0).to_radians(),
        };

        let distance = planet::earth::geodesic_dist(&paris, &washington);
        ret.push_str(&format!("巴黎到华盛顿距离: {} 米\n", distance));

        // --- 5. 坐标转换 ---
        let right_ascension = 116.328942_f64.to_radians();
        let declination = 28.026183_f64.to_radians();
        let oblq_eclip = ecliptic::mn_oblq_IAU(julian_ephm_day);

        // 修正：新版 astro 库可能不再提供 ecl_frm_eq! 宏，建议直接使用函数
        let (ecl_long, ecl_lat) = (
            coords::ecl_long_frm_eq(right_ascension, declination, oblq_eclip),
            coords::ecl_lat_frm_eq(right_ascension, declination, oblq_eclip),
        );
        ret.push_str(&format!(
            "Pollux 恒星黄道坐标: 经度 {}, 纬度 {}\n",
            ecl_long, ecl_lat
        ));

        // --- 6. 章动 (Nutation) ---
        let (nut_in_long, nut_in_oblq) = nutation::nutation(julian_ephm_day);
        ret.push_str(&format!(
            "黄经章动: {}, 倾角章动: {}\n",
            nut_in_long, nut_in_oblq
        ));
    }

    Ok(ret)
}

fn get_zodiac_sign(lon: f64) -> &'static str {
    // 标准黄道星座划分，每 30 度一个
    let signs = [
        "白羊座 (Aries)",
        "金牛座 (Taurus)",
        "双子座 (Gemini)",
        "巨蟹座 (Cancer)",
        "狮子座 (Leo)",
        "处女座 (Virgo)",
        "天秤座 (Libra)",
        "天蝎座 (Scorpio)",
        "射手座 (Sagittarius)",
        "摩羯座 (Capricorn)",
        "水瓶座 (Aquarius)",
        "双鱼座 (Pisces)",
    ];
    let index = (lon / 30.0).floor() as usize;
    signs[index % 12]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_time_getter() {
        println!("当前时间信息: \n\n{}", time_info_getter().await.unwrap());
    }
}
