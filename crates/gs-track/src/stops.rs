#![forbid(unsafe_code)]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopState { Candidate, Stopped, None }

#[derive(Debug, Clone, Copy)]
pub enum StopEvent { Started, Ended }

#[derive(Debug, Clone)]
pub struct StopDetector {
    pub radius_m: f64,
    pub min_duration_s: f64,
    pub exit_confirm: usize,
    // candidate state
    pub c_lat: f64,
    pub c_lon: f64,
    pub c_sum_lat: f64,
    pub c_sum_lon: f64,
    pub c_count: usize,
    pub c_started_ms: Option<u64>,
    pub state: StopState,
    pub consecutive_outside: usize,
    pub events: Vec<StopEvent>,
}

impl StopDetector {
    pub fn new(radius_m: f64, min_duration_s: f64, exit_confirm: usize) -> Self {
        Self {
            radius_m, min_duration_s, exit_confirm,
            c_lat: 0.0, c_lon: 0.0,
            c_sum_lat: 0.0, c_sum_lon: 0.0, c_count: 0,
            c_started_ms: None,
            state: StopState::None,
            consecutive_outside: 0,
            events: Vec::new(),
        }
    }

    pub fn state(&self) -> StopState { self.state }
    pub fn take_events(&mut self) -> Vec<StopEvent> { std::mem::take(&mut self.events) }

    /// Feed a new estimated position (ENU meters for distance, or use lat/lon haversine).
    /// Here we accept lat/lon and compute distance via origin.
    pub fn update(&mut self, lat: f64, lon: f64, origin_lat: f64, origin_lon: f64, now_ms: u64) {
        // effective radius
        let r_eff = self.radius_m; // + min(acc, radius) would need accuracy; kept simple
        // centroid
        if self.state == StopState::None || self.state == StopState::Stopped && self.consecutive_outside >= self.exit_confirm {
            // start candidate
            self.c_lat = lat; self.c_lon = lon;
            self.c_sum_lat = lat; self.c_sum_lon = lon; self.c_count = 1;
            self.c_started_ms = Some(now_ms);
            self.state = StopState::Candidate;
            self.consecutive_outside = 0;
            return;
        }
        // distance from candidate centroid to new point
        let d = gs_geo::haversine_distance(
            gs_geo::LatLon::new(self.c_sum_lat / self.c_count as f64, self.c_sum_lon / self.c_count as f64),
            gs_geo::LatLon::new(lat, lon),
        );
        if d <= r_eff {
            self.c_sum_lat += lat; self.c_sum_lon += lon; self.c_count += 1;
            self.consecutive_outside = 0;
            // check duration
            if let Some(start) = self.c_started_ms {
                let dur = (now_ms - start) as f64 / 1000.0;
                if dur >= self.min_duration_s && self.state == StopState::Candidate {
                    self.state = StopState::Stopped;
                    self.events.push(StopEvent::Started);
                }
            }
        } else {
            self.consecutive_outside += 1;
            if self.state == StopState::Stopped && self.consecutive_outside >= self.exit_confirm {
                self.events.push(StopEvent::Ended);
                // restart candidate at last point
                self.c_lat = lat; self.c_lon = lon;
                self.c_sum_lat = lat; self.c_sum_lon = lon; self.c_count = 1;
                self.c_started_ms = Some(now_ms);
                self.state = StopState::Candidate;
                self.consecutive_outside = 0;
            }
        }
        let _ = (origin_lat, origin_lon);
    }
}
