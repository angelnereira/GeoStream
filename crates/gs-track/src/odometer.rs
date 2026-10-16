#![forbid(unsafe_code)]

use gs_geo::{LatLon, haversine_distance};

#[derive(Debug, Clone, Copy)]
pub struct Odometer {
    pub anchor_lat: f64,
    pub anchor_lon: f64,
    pub value_m: f64,
    pub bridged_m: f64,
    pub has_anchor: bool,
}

impl Default for Odometer {
    fn default() -> Self { Self::new() }
}
impl Odometer {
    pub fn new() -> Self { Self { anchor_lat: 0.0, anchor_lon: 0.0, value_m: 0.0, bridged_m: 0.0, has_anchor: false } }
    pub fn reset(&mut self) { self.value_m = 0.0; self.bridged_m = 0.0; self.has_anchor = false; }
    pub fn value(&self) -> f64 { self.value_m }

    pub fn feed(&mut self, lat: f64, lon: f64, pos_std_m: f64, factor: f64) {
        if !self.has_anchor {
            self.anchor_lat = lat; self.anchor_lon = lon; self.has_anchor = true; return;
        }
        let dist = haversine_distance(
            LatLon::new(self.anchor_lat, self.anchor_lon),
            LatLon::new(lat, lon),
        );
        let threshold = factor * pos_std_m;
        if dist >= threshold {
            self.value_m += dist;
            self.anchor_lat = lat; self.anchor_lon = lon;
        }
    }

    pub fn bridge(&mut self, gap_m: f64) {
        self.value_m += gap_m;
        self.bridged_m += gap_m;
    }
}
