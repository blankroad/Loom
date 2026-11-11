//! `SimPsu`: a deterministic bench supply. Settings, output, protection (OVP/OCP/OTP), CV/CC,
//! a resistive load with inrush at switch-on, seeded noise. Time is passed in (ns), so tests
//! drive it with a simulated clock.

use serde_json::{Value, json};

use crate::model::Limits;

/// Over-temperature: output power above this (W) …
pub const OTP_POWER_W: f64 = 150.0;
/// … for longer than this trips OTP.
pub const OTP_AFTER_NS: u64 = 2_000_000_000;

/// Protection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Prot {
    /// No trip.
    Ok,
    /// Over-voltage.
    Ovp,
    /// Over-current.
    Ocp,
    /// Over-temperature.
    Otp,
}

impl Prot {
    /// Wire value.
    pub fn as_str(self) -> &'static str {
        match self {
            Prot::Ok => "ok",
            Prot::Ovp => "ovp",
            Prot::Ocp => "ocp",
            Prot::Otp => "otp",
        }
    }
}

/// Regulation mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Constant voltage.
    Cv,
    /// Constant current (current limit reached).
    Cc,
    /// Output off.
    Off,
}

impl Mode {
    /// Wire value.
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Cv => "cv",
            Mode::Cc => "cc",
            Mode::Off => "off",
        }
    }
}

/// Load on the output.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Load {
    /// Steady resistance (Ω).
    pub r_ohm: f64,
    /// Extra current right after switch-on (A), decaying with `inrush_tau_ms`.
    pub inrush_a: f64,
    /// Inrush time constant (ms).
    pub inrush_tau_ms: f64,
    /// Current noise amplitude (A, uniform ±).
    pub noise_a: f64,
}

impl Default for Load {
    /// A small ECU at 13.5 V draws ≈ 0.34 A (explore plan §5.1 example).
    fn default() -> Self {
        Load {
            r_ohm: 40.0,
            inrush_a: 1.5,
            inrush_tau_ms: 50.0,
            noise_a: 0.002,
        }
    }
}

/// One measurement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Meas {
    /// Volts.
    pub v: f64,
    /// Amperes.
    pub i: f64,
    /// Mode at sampling time.
    pub mode: Mode,
}

/// The simulated supply.
#[derive(Debug, Clone)]
pub struct SimPsu {
    /// Set voltage.
    pub set_v: f64,
    /// Set current limit.
    pub set_i: f64,
    /// Output enabled.
    pub out: bool,
    /// Protection state (a trip switches the output off; `out on` clears it).
    pub prot: Prot,
    /// OVP threshold (V).
    pub ovp_v: f64,
    /// OCP enabled: reaching the current limit trips instead of regulating.
    pub ocp: bool,
    /// Load model.
    pub load: Load,
    /// Profile limits enforced by the supply too (defence in depth).
    pub limits: Limits,
    on_since_ns: Option<u64>,
    hot_since_ns: Option<u64>,
    last_mode: Mode,
    rng: u64,
}

impl SimPsu {
    /// New supply, output off, 0 V / 1 A, deterministic noise from `seed`.
    pub fn new(limits: Limits, seed: u64) -> SimPsu {
        SimPsu {
            set_v: 0.0,
            set_i: 1.0,
            out: false,
            prot: Prot::Ok,
            ovp_v: limits.vmax * 1.1,
            ocp: false,
            load: Load::default(),
            limits,
            on_since_ns: None,
            hot_since_ns: None,
            last_mode: Mode::Off,
            rng: seed | 1,
        }
    }

    /// `*IDN?` answer.
    pub fn idn(&self) -> &'static str {
        "Loom,SimPsu,0,1.0"
    }

    fn noise(&mut self) -> f64 {
        // xorshift64*: deterministic, uniform in [-1, 1).
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        let x = self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11;
        (x as f64 / (1u64 << 53) as f64) * 2.0 - 1.0
    }

    /// Apply settings (limits checked; over → `Err`, nothing changes).
    pub fn set(&mut self, volt: Option<f64>, curr: Option<f64>) -> Result<(), String> {
        for x in [volt, curr].into_iter().flatten() {
            if !x.is_finite() || x < 0.0 {
                return Err(format!("bad value {x}"));
            }
        }
        if let Some(why) = self.limits.violation(volt, curr) {
            return Err(why);
        }
        if let Some(v) = volt {
            self.set_v = v;
        }
        if let Some(i) = curr {
            self.set_i = i;
        }
        Ok(())
    }

    /// Switch the output. `on` clears a protection trip (like the front-panel reset).
    pub fn output(&mut self, on: bool, now_ns: u64) {
        if on {
            self.prot = Prot::Ok;
            if !self.out {
                self.on_since_ns = Some(now_ns);
            }
            if self.set_v > self.ovp_v {
                self.trip(Prot::Ovp);
                return;
            }
        } else {
            self.on_since_ns = None;
            self.hot_since_ns = None;
        }
        self.out = on;
    }

    fn trip(&mut self, p: Prot) {
        self.prot = p;
        self.out = false;
        self.on_since_ns = None;
        self.hot_since_ns = None;
    }

    /// Current mode (last measured).
    pub fn mode(&self) -> Mode {
        if self.out { self.last_mode } else { Mode::Off }
    }

    /// Measure at `now_ns` (advances the noise source; may trip OCP/OTP).
    pub fn measure(&mut self, now_ns: u64) -> Meas {
        if !self.out {
            self.last_mode = Mode::Off;
            return Meas {
                v: 0.0,
                i: 0.0,
                mode: Mode::Off,
            };
        }
        let since = now_ns.saturating_sub(self.on_since_ns.unwrap_or(now_ns)) as f64 / 1e6;
        let inrush = self.load.inrush_a * (-since / self.load.inrush_tau_ms.max(1e-3)).exp();
        let i_cv = self.set_v / self.load.r_ohm.max(1e-6) + inrush;
        let (mut v, mut i, mode) = if i_cv > self.set_i {
            (self.set_v.min(self.set_i * self.load.r_ohm), self.set_i, Mode::Cc)
        } else {
            (self.set_v, i_cv, Mode::Cv)
        };
        if mode == Mode::Cc && self.ocp {
            self.trip(Prot::Ocp);
            self.last_mode = Mode::Off;
            return Meas {
                v: 0.0,
                i: 0.0,
                mode: Mode::Off,
            };
        }
        if v * i > OTP_POWER_W {
            let hot = *self.hot_since_ns.get_or_insert(now_ns);
            if now_ns.saturating_sub(hot) > OTP_AFTER_NS {
                self.trip(Prot::Otp);
                self.last_mode = Mode::Off;
                return Meas {
                    v: 0.0,
                    i: 0.0,
                    mode: Mode::Off,
                };
            }
        } else {
            self.hot_since_ns = None;
        }
        i = (i + self.noise() * self.load.noise_a).max(0.0);
        v = (v + self.noise() * 0.001).max(0.0);
        self.last_mode = mode;
        Meas { v, i, mode }
    }

    /// `psu.state` payload (without `ch`).
    pub fn state_json(&self) -> Value {
        json!({"set_v": self.set_v, "set_i": self.set_i, "out": self.out, "prot": self.prot.as_str(),
               "mode": self.mode().as_str(), "vmax": self.limits.vmax, "imax": self.limits.imax})
    }
}
