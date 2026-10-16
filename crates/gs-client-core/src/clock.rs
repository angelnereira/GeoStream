#![forbid(unsafe_code)]

#[derive(Debug, Clone)]
pub struct ClockSync {
    pub offset_ms: Option<i64>,
    pub rtt_ms: Option<u64>,
    pub alpha: f64,
    pub rtt_reject_ms: u64,
    pub max_rtt_in_window_ms: Option<u64>,
}

impl ClockSync {
    pub fn new() -> Self {
        Self {
            offset_ms: None,
            rtt_ms: None,
            alpha: 0.1,
            rtt_reject_ms: 500,
            max_rtt_in_window_ms: None,
        }
    }

    pub fn sample(&mut self, t0_ms: u64, t1_ms: u64, ts_ms: u64) -> SampleResult {
        if t1_ms <= t0_ms {
            return SampleResult::Rejected;
        }
        let rtt_ms = t1_ms - t0_ms;
        if rtt_ms > self.rtt_reject_ms {
            return SampleResult::Rejected;
        }
        let now_device_mid = t0_ms + rtt_ms / 2;
        let raw_offset = ts_ms as i64 - now_device_mid as i64;
        let old = self.offset_ms.unwrap_or(raw_offset);
        let smoothed = (old as f64 * (1.0 - self.alpha) + raw_offset as f64 * self.alpha) as i64;
        self.offset_ms = Some(smoothed);
        self.rtt_ms = Some(rtt_ms);
        SampleResult::Accepted {
            offset_ms: smoothed,
            rtt_ms,
        }
    }

    pub fn server_now_ms(&self, device_ms: u64) -> u64 {
        match self.offset_ms {
            Some(offset) => (device_ms as i64 + offset).max(0) as u64,
            None => device_ms,
        }
    }

    pub fn has_offset(&self) -> bool {
        self.offset_ms.is_some()
    }
}

impl Default for ClockSync {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleResult {
    Accepted { offset_ms: i64, rtt_ms: u64 },
    Rejected,
}
