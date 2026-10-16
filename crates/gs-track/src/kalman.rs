//! Constant-velocity Kalman filter, 4x4, arrays fijos (22 §2.3).

#![forbid(unsafe_code)]

#[derive(Debug, Clone, Copy)]
pub struct KalmanCv {
    /// state [e,n,ve,vn]
    x: [f64; 4],
    /// covariance (row-major)
    p: [f64; 16],
}

#[derive(Debug, Clone, Copy)]
pub struct KalmanStatus {
    pub position_std_m: f64,
    pub mahalanobis_d2: f64,
}

impl KalmanCv {
    pub fn init(enu: (f64, f64), sigma_v0: f64) -> Self {
        let mut s = Self {
            x: [enu.0, enu.1, 0.0, 0.0],
            p: [0.0; 16],
        };
        // P diag: pos variance = sigma_m^2? We start with a sane prior.
        // Position uncertainty = 8? Use 1 m default for prior pos variance.
        s.p[0] = 1.0;
        s.p[5] = 1.0;
        s.p[10] = sigma_v0 * sigma_v0;
        s.p[15] = sigma_v0 * sigma_v0;
        s
    }

    pub fn set_last(&mut self, enu: (f64, f64)) {
        self.x[0] = enu.0;
        self.x[1] = enu.1;
    }

    pub fn state(&self) -> (f64, f64, f64, f64) {
        (self.x[0], self.x[1], self.x[2], self.x[3])
    }
    pub fn position_std(&self) -> f64 {
        self.p[0].sqrt().max(1e-3)
    }

    /// One step. Returns Mahalanobis d2; side effect marks outlier if > chi2.
    pub fn step(
        &mut self,
        meas_enu: (f64, f64),
        sample_speed: Option<f64>,
        sample_heading: Option<f64>,
        sample_speed_acc: Option<f64>,
        dt_s: f64,
        sigma_a: f64,
        accuracy_m: f64,
        sigma_factor: f64,
        chi2: f64,
        outlier_reset_count: usize,
        outlier_run: &mut usize,
        outlier_jump: &mut bool,
    ) -> f64 {
        let dt = if dt_s.is_finite() && dt_s > 0.0 {
            dt_s
        } else {
            1e-3
        };
        // Predict
        // F
        let f = [
            1.0, 0.0, dt, 0.0, 0.0, 1.0, 0.0, dt, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ];
        // x = F x
        let mut xn = [0.0; 4];
        for i in 0..4 {
            for j in 0..4 {
                xn[i] += f[i * 4 + j] * self.x[j];
            }
        }
        self.x = xn;
        // P = F P F^T + Q (Q discrete WNA)
        let mut pnew = [0.0; 16];
        for i in 0..4 {
            for j in 0..4 {
                let mut s = 0.0;
                for a in 0..4 {
                    for b in 0..4 {
                        s += f[i * 4 + a] * self.p[a * 4 + b] * f[j * 4 + b];
                    }
                }
                pnew[i * 4 + j] = s;
            }
        }
        // Q per axis (pos/vel coupling)
        let sa2 = sigma_a * sigma_a;
        let dt2 = dt * dt;
        let dt3 = dt2 * dt;
        // east
        pnew[0] += sa2 * dt3 / 3.0;
        pnew[2] += sa2 * dt2 / 2.0;
        pnew[8] += sa2 * dt2 / 2.0;
        pnew[10] += sa2 * dt;
        // north
        pnew[5] += sa2 * dt3 / 3.0;
        pnew[7] += sa2 * dt2 / 2.0;
        pnew[13] += sa2 * dt2 / 2.0;
        pnew[15] += sa2 * dt;
        self.p = pnew;

        // Measurement z for position
        // S = H P H^T + R (position meas H=[1,0,0,0;0,1,0,0])
        // H P H^T is top-left 2x2 of P
        let s00 = self.p[0] + (sigma_factor * accuracy_m).powi(2);
        let s01 = self.p[1];
        let s10 = self.p[4];
        let s11 = self.p[5] + (sigma_factor * accuracy_m).powi(2);

        let det = s00 * s11 - s01 * s10;
        if det <= 0.0 {
            return 0.0;
        }
        let inv00 = s11 / det;
        let inv01 = -s01 / det;
        let inv10 = -s10 / det;
        let inv11 = s00 / det;

        // innovation
        let ye0 = meas_enu.0 - self.x[0];
        let ye1 = meas_enu.1 - self.x[1];
        let d2 = ye0 * ye0 * inv00 + ye0 * ye1 * inv01 + ye1 * ye0 * inv10 + ye1 * ye1 * inv11;

        // gate
        if d2 > chi2 {
            *outlier_jump = true;
            *outlier_run += 1;
            if *outlier_run >= outlier_reset_count {
                // reset in measurement
                self.x[0] = meas_enu.0;
                self.x[1] = meas_enu.1;
                // keep velocity as is
                self.p[0] = (sigma_factor * accuracy_m).powi(2);
                self.p[5] = (sigma_factor * accuracy_m).powi(2);
                self.p[10] = 100.0;
                self.p[15] = 100.0;
                *outlier_run = 0;
            }
            return d2;
        }

        // Joseph update with position measurement
        // H = [[1,0,0,0],[0,1,0,0]]
        // K = P H^T S^-1 (4x2)
        // PHt (4x2): column j = P[:,j] for j in 0,1
        let mut ph0 = [0.0; 4];
        let mut ph1 = [0.0; 4];
        for i in 0..4 {
            ph0[i] = self.p[i * 4 + 0];
            ph1[i] = self.p[i * 4 + 1];
        }
        let mut k0 = [0.0; 4];
        let mut k1 = [0.0; 4];
        for i in 0..4 {
            k0[i] = ph0[i] * inv00 + ph1[i] * inv10;
            k1[i] = ph0[i] * inv01 + ph1[i] * inv11;
        }
        // innovation: innovation (2) -> [ye0,ye1]
        // x += K * y (4x2 times 2)
        self.x[0] += k0[0] * ye0 + k1[0] * ye1;
        self.x[1] += k0[1] * ye0 + k1[1] * ye1;
        self.x[2] += k0[2] * ye0 + k1[2] * ye1;
        self.x[3] += k0[3] * ye0 + k1[3] * ye1;

        // P' = (I - K H) P (I - K H)^T + K R K^T
        // KH (4x4): K (4x2) * H (2x4) => only cols 0,1 of KH filled = K.
        // M = I - K H
        let mut m = [[0.0f64; 4]; 4];
        for i in 0..4 {
            for j in 0..4 {
                m[i][j] = if i == j { 1.0 } else { 0.0 };
            }
        }
        // K*H: K[:,0] at col0; K[:,1] at col1
        for i in 0..4 {
            m[i][0] -= k0[i];
            m[i][1] -= k1[i];
        }
        // M P M^T
        let mut pm = [0.0; 16];
        // P (current before update, but we mutated x already; P unchanged)
        for i in 0..4 {
            for j in 0..4 {
                let mut s = 0.0;
                for a in 0..4 {
                    for b in 0..4 {
                        // need P old; we only have self.p
                        s += m[i][a] * self.p[a * 4 + b] * m[j][b];
                    }
                }
                pm[i * 4 + j] = s;
            }
        }
        // + K R K^T
        let r00 = (sigma_factor * accuracy_m).powi(2);
        let r11 = r00;
        // K (4x2) R K^T = r00 * k0*k0^T + r11*k1*k1^T
        for i in 0..4 {
            for j in 0..4 {
                self.p[i * 4 + j] = pm[i * 4 + j] + r00 * k0[i] * k0[j] + r11 * k1[i] * k1[j];
            }
        }

        // velocity measurement update (optional) omitted for brevity but:
        if let (Some(s), Some(hdeg), Some(sa)) = (sample_speed, sample_heading, sample_speed_acc) {
            if s >= 1.0 {
                let theta = hdeg.to_radians();
                let zv0 = s * theta.sin();
                let zv1 = s * theta.cos();
                // H_v = velocity; measurement residual
                let h0 = self.x[2] - zv0;
                let h1 = self.x[3] - zv1;
                let rv = (sa * sa).max(1e-6);
                // simple scalar-ish Kalman with diag
                let gain0 = self.p[2 * 4 + 2] / (self.p[2 * 4 + 2] + rv);
                let gain1 = self.p[3 * 4 + 3] / (self.p[3 * 4 + 3] + rv);
                self.x[2] -= gain0 * h0;
                self.x[3] -= gain1 * h1;
            }
        }

        // successful real update resets outlier_run
        *outlier_run = 0;
        d2
    }

    /// Pseudo-measurement of zero velocity (ZUPT): pulls velocities toward 0.
    pub fn zupt(&mut self, sigma: f64) {
        // measurement z = [0,0] on vx,vy with R = sigma^2 I.
        let rv = (sigma * sigma).max(1e-6);
        for axis in 0..2 {
            let idx = 2 + axis;
            let gain = self.p[idx * 4 + idx] / (self.p[idx * 4 + idx] + rv);
            self.x[idx] -= gain * self.x[idx];
            self.p[idx * 4 + idx] *= 1.0 - gain;
        }
    }
}

// param-less config dummy: TrackConfig not accessible here without circular; add field later
// We don't actually reference config in KalmanCv, so remove config? config used above for outlier reset p[10]=sigma_v0^2 but we set default. Remove reference.
