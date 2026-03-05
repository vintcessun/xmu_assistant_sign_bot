const EARTH_RADIUS: f64 = 6_371_000.0;

#[derive(Debug, Clone, Copy)]
pub struct GeoPoint {
    pub lat: f32,
    pub lon: f32,
    pub dist: f64,
}
/// 核心方法：通过三个点及距离计算唯一位置，返回目标点的 GeoPoint
pub fn location_trilaterate(p1: GeoPoint, p2: GeoPoint, p3: GeoPoint) -> Option<GeoPoint> {
    // 1. 调用两圆求交点逻辑（基于前两个点）
    let candidates = solve_two_circles(
        p1.lat as f64,
        p1.lon as f64,
        p1.dist,
        p2.lat as f64,
        p2.lon as f64,
        p2.dist,
    )?;

    // 2. 使用第三个点 p3 进行验证，找出误差最小的点
    let mut best_coords = candidates[0];
    let mut min_error = f64::MAX;

    for cand in candidates.iter() {
        let d_calc = haversine_distance(cand.0, cand.1, p3.lat as f64, p3.lon as f64);
        let error = (d_calc - p3.dist).abs();

        if error < min_error {
            min_error = error;
            best_coords = *cand;
        }
    }

    // 如果最小误差依然很大（比如 > 100米），说明数据点可能冲突或无法交汇
    if min_error > 100.0 {
        return None;
    }

    // 3. 返回封装好的 GeoPoint，目标点到自身的距离设为 0.0
    Some(GeoPoint {
        lat: best_coords.0 as f32,
        lon: best_coords.1 as f32,
        dist: 0.0,
    })
}

fn _latlon_to_xy(lat: f64, lon: f64, lat0: f64, lon0: f64) -> (f64, f64) {
    let x = (lon - lon0).to_radians() * EARTH_RADIUS * lat0.to_radians().cos();
    let y = (lat - lat0).to_radians() * EARTH_RADIUS;
    (x, y)
}

fn _xy_to_latlon(x: f64, y: f64, lat0: f64, lon0: f64) -> (f64, f64) {
    let lat = lat0 + y.to_degrees() / EARTH_RADIUS;
    let lon = lon0 + x.to_degrees() / (EARTH_RADIUS * lat0.to_radians().cos());
    (lat, lon)
}

fn solve_two_circles(
    lat1: f64,
    lon1: f64,
    d1: f64,
    lat2: f64,
    lon2: f64,
    d2: f64,
) -> Option<[(f64, f64); 2]> {
    let lat0 = (lat1 + lat2) / 2.0;
    let lon0 = (lon1 + lon2) / 2.0;
    let (x1, y1) = _latlon_to_xy(lat1, lon1, lat0, lon0);
    let (x2, y2) = _latlon_to_xy(lat2, lon2, lat0, lon0);

    let d_centers = ((x2 - x1).powi(2) + (y2 - y1).powi(2)).sqrt();

    if d_centers > d1 + d2 || d_centers < (d1 - d2).abs() {
        return None;
    }

    let a = (d1.powi(2) - d2.powi(2) + d_centers.powi(2)) / (2.0 * d_centers);
    let h_sq = d1.powi(2) - a.powi(2);
    if h_sq < 0.0 {
        return None;
    }
    let h = h_sq.sqrt();

    let xm = x1 + a * (x2 - x1) / d_centers;
    let ym = y1 + a * (y2 - y1) / d_centers;

    let rx = -(y2 - y1) * (h / d_centers);
    let ry = (x2 - x1) * (h / d_centers);

    Some([
        _xy_to_latlon(xm + rx, ym + ry, lat0, lon0),
        _xy_to_latlon(xm - rx, ym - ry, lat0, lon0),
    ])
}

fn haversine_distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let d_lat = (lat2 - lat1).to_radians();
    let d_lon = (lon2 - lon1).to_radians();
    let a = (d_lat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (d_lon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    EARTH_RADIUS * c
}
