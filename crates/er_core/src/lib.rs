//! Enc Res core: joint encoder resolution and velocity-quantization model (pure Rust, no I/O).
//!
//! All angles are joint (output) angles unless noted. Speeds are deg/s at the output.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Incremental,
    Absolute,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mount {
    Motor,
    Output,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Status {
    Pass,
    Warn,
    Fail,
}

impl Status {
    pub fn key(self) -> &'static str {
        match self {
            Status::Pass => "pass",
            Status::Warn => "warn",
            Status::Fail => "fail",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Design {
    pub kind: Kind,
    pub mount: Mount,
    /// Incremental lines per revolution (PPR).
    pub lines: f64,
    /// Quadrature decoding factor: 1, 2 or 4.
    pub decode: f64,
    /// Absolute single-turn bits.
    pub bits: f64,
    pub ratio: f64,
    pub backlash_arcmin: f64,
    pub arm_mm: f64,
    pub rate_hz: f64,
    pub max_speed_dps: f64,
    /// Interpolation noise, counts RMS.
    pub noise_counts: f64,
    pub work_speed_dps: f64,
    pub target_pct: f64,
    pub decoder_limit_hz: f64,
    /// Timer clock used by period (T-method) measurement.
    pub timer_hz: f64,
}

/// Numeric fields: (id, min, max), inclusive. The web UI uses the same ids for its inputs.
pub const FIELDS: [(&str, f64, f64); 13] = [
    ("lines", 1.0, 1e7),
    ("decode", 1.0, 4.0),
    ("bits", 1.0, 32.0),
    ("ratio", 0.01, 1e4),
    ("backlash_arcmin", 0.0, 600.0),
    ("arm_mm", 0.1, 1e5),
    ("rate_hz", 1.0, 1e6),
    ("max_speed_dps", 1e-3, 1e6),
    ("noise_counts", 0.0, 1e3),
    ("work_speed_dps", 1e-4, 1e6),
    ("target_pct", 0.01, 100.0),
    ("decoder_limit_hz", 1.0, 1e10),
    ("timer_hz", 1.0, 1e10),
];

pub fn bounds(id: &str) -> Option<(f64, f64)> {
    FIELDS.iter().find(|f| f.0 == id).map(|f| (f.1, f.2))
}

impl Default for Design {
    fn default() -> Self {
        Preset::HarmonicMotor.design()
    }
}

impl Design {
    pub fn get(&self, id: &str) -> f64 {
        match id {
            "lines" => self.lines,
            "decode" => self.decode,
            "bits" => self.bits,
            "ratio" => self.ratio,
            "backlash_arcmin" => self.backlash_arcmin,
            "arm_mm" => self.arm_mm,
            "rate_hz" => self.rate_hz,
            "max_speed_dps" => self.max_speed_dps,
            "noise_counts" => self.noise_counts,
            "work_speed_dps" => self.work_speed_dps,
            "target_pct" => self.target_pct,
            "decoder_limit_hz" => self.decoder_limit_hz,
            "timer_hz" => self.timer_hz,
            _ => f64::NAN,
        }
    }

    pub fn set(&mut self, id: &str, v: f64) {
        let slot = match id {
            "lines" => &mut self.lines,
            "decode" => &mut self.decode,
            "bits" => &mut self.bits,
            "ratio" => &mut self.ratio,
            "backlash_arcmin" => &mut self.backlash_arcmin,
            "arm_mm" => &mut self.arm_mm,
            "rate_hz" => &mut self.rate_hz,
            "max_speed_dps" => &mut self.max_speed_dps,
            "noise_counts" => &mut self.noise_counts,
            "work_speed_dps" => &mut self.work_speed_dps,
            "target_pct" => &mut self.target_pct,
            "decoder_limit_hz" => &mut self.decoder_limit_hz,
            "timer_hz" => &mut self.timer_hz,
            _ => return,
        };
        *slot = v;
    }

    /// Ids of fields that are out of range (or NaN/inf). Fields irrelevant to the encoder kind are skipped.
    pub fn invalid(&self) -> Vec<&'static str> {
        FIELDS
            .iter()
            .filter(|(id, ..)| match self.kind {
                Kind::Incremental => *id != "bits",
                Kind::Absolute => !matches!(*id, "lines" | "decode" | "decoder_limit_hz"),
            })
            .filter(|(id, lo, hi)| {
                let v = self.get(id);
                let bad = !(v.is_finite() && v >= *lo && v <= *hi);
                bad || (*id == "decode" && ![1.0, 2.0, 4.0].contains(&v)) || (*id == "bits" && v.fract() != 0.0)
            })
            .map(|f| f.0)
            .collect()
    }

    /// Counts per encoder-shaft revolution.
    pub fn counts_per_encoder_rev(&self) -> f64 {
        match self.kind {
            Kind::Incremental => self.lines.round() * self.decode,
            Kind::Absolute => 2f64.powf(self.bits.round()),
        }
    }

    /// Encoder-shaft revolutions per output revolution.
    pub fn shaft_factor(&self) -> f64 {
        match self.mount {
            Mount::Motor => self.ratio,
            Mount::Output => 1.0,
        }
    }

    pub fn report(&self) -> Report {
        let counts_enc = self.counts_per_encoder_rev();
        let counts_out = counts_enc * self.shaft_factor();
        let res_deg = 360.0 / counts_out;
        let res_rad = res_deg.to_radians();
        let tip_step_um = self.arm_mm * res_rad * 1000.0;
        let backlash_tip_um = self.arm_mm * (self.backlash_arcmin / 60.0).to_radians() * 1000.0;
        let dominant = if self.backlash_arcmin <= 0.0 {
            Dominant::QuantOnly
        } else if backlash_tip_um > tip_step_um {
            Dominant::Backlash
        } else {
            Dominant::Quantization
        };
        let quantum_dps = res_deg * self.rate_hz;
        // A difference of two noisy samples has sqrt(2) times the single-sample noise.
        let noise_factor = 1.0 + std::f64::consts::SQRT_2 * self.noise_counts;
        let m_err_pct = m_error_pct(quantum_dps * noise_factor, self.work_speed_dps);
        let t_err_pct = t_error_pct(res_deg, self.timer_hz, self.work_speed_dps);
        let crossover_dps = res_deg * (self.rate_hz * self.timer_hz).sqrt();
        let shaft_rps = self.max_speed_dps / 360.0 * self.shaft_factor();
        let edge_hz = self.max_speed_dps / 360.0 * counts_out;
        let rev_per_sample = shaft_rps / self.rate_hz;
        let (count_util_pct, count_status) = match self.kind {
            Kind::Incremental => {
                let u = edge_hz / self.decoder_limit_hz * 100.0;
                (Some(u), if u > 100.0 { Status::Fail } else if u > 80.0 { Status::Warn } else { Status::Pass })
            }
            // Absolute: wrap tracking between frames is ambiguous at >= half a turn per sample.
            Kind::Absolute => (
                None,
                if rev_per_sample >= 0.5 { Status::Fail } else if rev_per_sample >= 0.25 { Status::Warn } else { Status::Pass },
            ),
        };
        Report {
            counts_enc,
            counts_out,
            res_deg,
            res_arcsec: res_deg * 3600.0,
            res_mrad: res_rad * 1000.0,
            tip_step_um,
            noise_tip_um: tip_step_um * self.noise_counts,
            backlash_tip_um,
            dominant,
            quantum_dps,
            quantum_eff_dps: quantum_dps * noise_factor,
            min_speed_dps: quantum_dps,
            crossover_dps,
            m_err_pct,
            t_err_pct,
            m_status: grade(m_err_pct, self.target_pct),
            t_status: grade(t_err_pct, self.target_pct),
            t_period_ms: res_deg / self.work_speed_dps * 1000.0,
            t_slow: res_deg / self.work_speed_dps > 1.0 / self.rate_hz,
            edge_hz,
            shaft_rpm: shaft_rps * 60.0,
            rev_per_sample,
            count_util_pct,
            count_status,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dominant {
    QuantOnly,
    Quantization,
    Backlash,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub counts_enc: f64,
    pub counts_out: f64,
    pub res_deg: f64,
    pub res_arcsec: f64,
    pub res_mrad: f64,
    pub tip_step_um: f64,
    pub noise_tip_um: f64,
    pub backlash_tip_um: f64,
    pub dominant: Dominant,
    /// M-method velocity quantum Δθ·f_s (deg/s).
    pub quantum_dps: f64,
    /// Quantum inflated by interpolation noise (1 + √2·σ).
    pub quantum_eff_dps: f64,
    /// Minimum speed giving at least one count per sample (= quantum).
    pub min_speed_dps: f64,
    /// Below this speed, period measurement (T-method) beats count difference (M-method).
    pub crossover_dps: f64,
    pub m_err_pct: f64,
    pub t_err_pct: f64,
    pub m_status: Status,
    pub t_status: Status,
    /// Time between two edges at working speed.
    pub t_period_ms: f64,
    /// True when the edge period exceeds the control period (T-method updates slower than the loop).
    pub t_slow: bool,
    pub edge_hz: f64,
    pub shaft_rpm: f64,
    pub rev_per_sample: f64,
    pub count_util_pct: Option<f64>,
    pub count_status: Status,
}

/// Worst-case M-method relative error (one quantum lost), capped at 100 %.
pub fn m_error_pct(quantum_dps: f64, speed_dps: f64) -> f64 {
    (quantum_dps / speed_dps * 100.0).min(100.0)
}

/// Worst-case T-method relative error (one timer tick per edge period), capped at 100 %.
pub fn t_error_pct(res_deg: f64, timer_hz: f64, speed_dps: f64) -> f64 {
    (speed_dps / (res_deg * timer_hz) * 100.0).min(100.0)
}

/// M-method reading by truncation: whole counts per sample times the quantum.
pub fn m_measured(quantum_dps: f64, speed_dps: f64) -> f64 {
    (speed_dps / quantum_dps).floor() * quantum_dps
}

pub fn grade(err_pct: f64, target_pct: f64) -> Status {
    if err_pct <= target_pct {
        Status::Pass
    } else if err_pct <= 2.0 * target_pct {
        Status::Warn
    } else {
        Status::Fail
    }
}

/// Result rows for the table and CSV: (key, English label, value, unit).
pub fn rows(d: &Design, r: &Report) -> Vec<(&'static str, &'static str, f64, &'static str)> {
    let mut v = vec![
        ("counts_enc", "Counts per encoder revolution", r.counts_enc, "counts"),
        ("counts_out", "Counts per output revolution", r.counts_out, "counts"),
        ("res_deg", "Output resolution", r.res_deg, "deg"),
        ("res_arcsec", "Output resolution", r.res_arcsec, "arcsec"),
        ("res_mrad", "Output resolution", r.res_mrad, "mrad"),
        ("tip_step", "Tool-tip step per count", r.tip_step_um, "um"),
        ("noise_tip", "Tool-tip noise (interpolation)", r.noise_tip_um, "um RMS"),
        ("backlash_tip", "Tool-tip backlash play", r.backlash_tip_um, "um"),
        ("quantum", "M-method velocity quantum", r.quantum_dps, "deg/s"),
        ("min_speed", "Min speed for 1 count/sample", r.min_speed_dps, "deg/s"),
        ("crossover", "T-method preferred below", r.crossover_dps, "deg/s"),
        ("m_err", "M-method error at working speed", r.m_err_pct, "%"),
        ("t_err", "T-method error at working speed", r.t_err_pct, "%"),
        ("t_period", "Edge period at working speed", r.t_period_ms, "ms"),
        ("edge_hz", "Count rate at max speed", r.edge_hz, "Hz"),
        ("shaft_rpm", "Encoder shaft speed at max", r.shaft_rpm, "rpm"),
        ("rev_sample", "Encoder revolutions per sample", r.rev_per_sample, "rev"),
    ];
    if let Some(u) = r.count_util_pct {
        v.push(("util", "Decoder utilisation", u, "%"));
    }
    v.push(("target", "Target error", d.target_pct, "%"));
    v
}

pub fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub fn to_csv(d: &Design, r: &Report) -> String {
    let mut out = String::from("quantity,value,unit\n");
    for (_, label, v, unit) in rows(d, r) {
        out.push_str(&format!("{},{},{}\n", csv_field(label), csv_field(&fmt(v)), csv_field(unit)));
    }
    out
}

/// About 4 significant digits, scientific outside [1e-3, 1e7).
pub fn fmt(v: f64) -> String {
    if !v.is_finite() {
        return "—".into();
    }
    let a = v.abs();
    if a != 0.0 && !(1e-3..1e7).contains(&a) {
        return format!("{:.3e}", v);
    }
    let dec = if a == 0.0 { 0 } else { (3 - a.log10().floor() as i32).clamp(0, 6) as usize };
    let s = format!("{:.*}", dec, v);
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Preset {
    QuasiDirect,
    HarmonicMotor,
    PlanetaryMotor,
}

impl Preset {
    pub const ALL: [Preset; 3] = [Preset::QuasiDirect, Preset::HarmonicMotor, Preset::PlanetaryMotor];

    pub fn key(self) -> &'static str {
        match self {
            Preset::QuasiDirect => "qdd",
            Preset::HarmonicMotor => "harmonic",
            Preset::PlanetaryMotor => "planetary",
        }
    }

    /// Synthetic, illustrative values only.
    pub fn design(self) -> Design {
        let base = Design {
            kind: Kind::Incremental,
            mount: Mount::Motor,
            lines: 2500.0,
            decode: 4.0,
            bits: 14.0,
            ratio: 100.0,
            backlash_arcmin: 1.0,
            arm_mm: 400.0,
            rate_hz: 1000.0,
            max_speed_dps: 180.0,
            noise_counts: 0.0,
            work_speed_dps: 5.0,
            target_pct: 5.0,
            decoder_limit_hz: 10e6,
            timer_hz: 10e6,
        };
        match self {
            Preset::HarmonicMotor => base,
            Preset::QuasiDirect => Design {
                kind: Kind::Absolute,
                mount: Mount::Output,
                bits: 14.0,
                ratio: 9.0,
                backlash_arcmin: 6.0,
                max_speed_dps: 720.0,
                noise_counts: 0.5,
                ..base
            },
            Preset::PlanetaryMotor => Design {
                kind: Kind::Absolute,
                mount: Mount::Motor,
                bits: 19.0,
                ratio: 50.0,
                backlash_arcmin: 10.0,
                max_speed_dps: 360.0,
                noise_counts: 1.0,
                ..base
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, rel: f64) -> bool {
        (a - b).abs() <= rel * b.abs()
    }

    #[test]
    fn incremental_2500x4_motor_n100() {
        let r = Preset::HarmonicMotor.design().report();
        assert_eq!(r.counts_enc, 10_000.0);
        assert_eq!(r.counts_out, 1_000_000.0);
        assert!(close(r.res_arcsec, 1.296, 1e-9));
    }

    #[test]
    fn absolute_14bit_output_side() {
        let d = Design { kind: Kind::Absolute, mount: Mount::Output, bits: 14.0, ..Design::default() };
        let r = d.report();
        assert_eq!(r.counts_out, 16384.0);
        assert!(close(r.res_deg, 0.02197, 1e-3));
    }

    #[test]
    fn tip_step_400mm() {
        // 2π/1e6 rad × 400 mm = 2.5133 µm.
        let r = Preset::HarmonicMotor.design().report();
        assert!(close(r.tip_step_um, 2.513274, 1e-6));
        // 1 arcmin backlash at 400 mm = 116.4 µm, dominates the step.
        assert!(close(r.backlash_tip_um, 116.355, 1e-4));
        assert_eq!(r.dominant, Dominant::Backlash);
    }

    #[test]
    fn m_method_quantum_and_min_speed_at_1khz() {
        let d = Design { kind: Kind::Absolute, mount: Mount::Output, bits: 14.0, ..Design::default() };
        let r = d.report();
        assert!(close(r.quantum_dps, 21.97266, 1e-6));
        assert_eq!(r.min_speed_dps, r.quantum_dps);
        // 5 deg/s working speed: below one count per sample -> capped at 100 %, FAIL.
        assert_eq!(r.m_err_pct, 100.0);
        assert_eq!(r.m_status, Status::Fail);
        assert_eq!(m_measured(r.quantum_dps, 5.0), 0.0);
        assert!(close(m_measured(r.quantum_dps, 50.0), 2.0 * r.quantum_dps, 1e-12));
    }

    #[test]
    fn count_rate_motor_side() {
        // 180 deg/s output = 0.5 rev/s output = 50 rev/s motor; × 10 000 counts = 500 kHz.
        let r = Preset::HarmonicMotor.design().report();
        assert!(close(r.edge_hz, 500_000.0, 1e-12));
        assert!(close(r.shaft_rpm, 3000.0, 1e-12));
        assert!(close(r.count_util_pct.unwrap_or(0.0), 5.0, 1e-12));
        assert_eq!(r.count_status, Status::Pass);
    }

    #[test]
    fn csv_escaping() {
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        let d = Design::default();
        let csv = to_csv(&d, &d.report());
        assert!(csv.starts_with("quantity,value,unit\nCounts per encoder revolution,10000,counts\n"));
    }

    #[test]
    fn invalid_inputs_are_reported_not_panicking() {
        for bad in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e12] {
            let mut d = Design::default();
            d.set("ratio", bad);
            d.set("rate_hz", bad);
            let inv = d.invalid();
            assert!(inv.contains(&"ratio") && inv.contains(&"rate_hz"), "{bad}: {inv:?}");
        }
        let mut d = Design::default();
        d.set("decode", 3.0);
        assert_eq!(d.invalid(), vec!["decode"]);
        assert!(Design::default().invalid().is_empty());
        assert!(Preset::ALL.iter().all(|p| p.design().invalid().is_empty()));
    }
}
