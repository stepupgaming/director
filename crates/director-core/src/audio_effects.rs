use std::f32::consts;
use std::fmt;

/// Constant-power pan law: given pan 0..1, returns (left_gain, right_gain).
pub fn pan_gains(pan: f32) -> (f32, f32) {
    let p = pan.clamp(0.0, 1.0);
    let angle = p * consts::FRAC_PI_2;
    (angle.cos(), angle.sin())
}

#[derive(Clone)]
pub struct AudioEffectChain {
    pub effects: Vec<Box<dyn AudioEffect>>,
}

impl fmt::Debug for AudioEffectChain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AudioEffectChain")
            .field("count", &self.effects.len())
            .finish()
    }
}

impl Default for AudioEffectChain {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioEffectChain {
    pub fn new() -> Self {
        Self {
            effects: Vec::new(),
        }
    }

    pub fn add(&mut self, effect: Box<dyn AudioEffect>) -> usize {
        let idx = self.effects.len();
        self.effects.push(effect);
        idx
    }

    pub fn get_mut(&mut self, idx: usize) -> Option<&mut Box<dyn AudioEffect>> {
        self.effects.get_mut(idx)
    }

    pub fn process(&mut self, samples: &mut [f32], sample_rate: u32, channels: u16) {
        for effect in &mut self.effects {
            effect.process(samples, sample_rate, channels);
        }
    }

    pub fn reset_all(&mut self) {
        for effect in &mut self.effects {
            effect.reset();
        }
    }

    pub fn len(&self) -> usize {
        self.effects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub trait AudioEffect: Send + fmt::Debug {
    fn process(&mut self, samples: &mut [f32], sample_rate: u32, channels: u16);
    fn reset(&mut self);
    fn set_parameter(&mut self, name: &str, value: f32);
    /// The `add_effect` name and the parameters `set_parameter` accepts.
    fn describe(&self) -> AudioEffectDescription;
    fn clone_box(&self) -> Box<dyn AudioEffect>;
}

/// An effect's name and settings, for the scene document.
pub struct AudioEffectDescription {
    pub kind: &'static str,
    /// EQ filter shape, such as `low_pass`.
    pub filter: Option<&'static str>,
    pub params: Vec<(&'static str, f32)>,
}

impl Clone for Box<dyn AudioEffect> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

// ============================================================================
// Reverb (Schroeder)
// ============================================================================

#[derive(Clone, Debug)]
pub struct Reverb {
    comb_delays: [CombFilter; 4],
    allpass1: AllPassFilter,
    allpass2: AllPassFilter,
    pub decay: f32,
    pub wet: f32,
    pub dry: f32,
}

#[derive(Clone, Debug)]
struct CombFilter {
    buffer: Vec<f32>,
    index: usize,
    _delay: usize,
    feedback: f32,
}

impl CombFilter {
    fn new(delay_samples: usize, feedback: f32) -> Self {
        Self {
            buffer: vec![0.0; delay_samples],
            index: 0,
            _delay: delay_samples,
            feedback,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let buf_len = self.buffer.len();
        let output = self.buffer[self.index];
        self.buffer[self.index] = input + output * self.feedback;
        self.index = (self.index + 1) % buf_len;
        output
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.index = 0;
    }
}

#[derive(Clone, Debug)]
struct AllPassFilter {
    buffer: Vec<f32>,
    index: usize,
    _delay: usize,
    feedback: f32,
}

impl AllPassFilter {
    fn new(delay_samples: usize, feedback: f32) -> Self {
        Self {
            buffer: vec![0.0; delay_samples],
            index: 0,
            _delay: delay_samples,
            feedback,
        }
    }

    fn process(&mut self, input: f32) -> f32 {
        let buf_len = self.buffer.len();
        let buf_out = self.buffer[self.index];
        let output = -self.feedback * input + buf_out;
        self.buffer[self.index] = input + self.feedback * buf_out;
        self.index = (self.index + 1) % buf_len;
        output
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.index = 0;
    }
}

impl Reverb {
    pub fn new(sample_rate: u32) -> Self {
        let base_delay = (sample_rate as f32 * 0.03) as usize;
        Self {
            comb_delays: [
                CombFilter::new(base_delay, 0.8),
                CombFilter::new((base_delay as f32 * 1.12) as usize, 0.8),
                CombFilter::new((base_delay as f32 * 1.36) as usize, 0.8),
                CombFilter::new((base_delay as f32 * 1.68) as usize, 0.8),
            ],
            allpass1: AllPassFilter::new((base_delay as f32 * 0.15) as usize, 0.6),
            allpass2: AllPassFilter::new((base_delay as f32 * 0.22) as usize, 0.6),
            decay: 0.5,
            wet: 0.3,
            dry: 0.7,
        }
    }

    #[allow(dead_code)]
    fn rebuild_feedback(&mut self, sample_rate: u32) {
        let rt60 = 3.0 * (1.0 - self.decay).max(0.01);
        let base_delay = (sample_rate as f32 * 0.03) as usize;
        let delays: [usize; 4] = [
            base_delay,
            (base_delay as f32 * 1.12) as usize,
            (base_delay as f32 * 1.36) as usize,
            (base_delay as f32 * 1.68) as usize,
        ];
        for (comb, &d) in self.comb_delays.iter_mut().zip(&delays) {
            let fb = (10.0_f32).powf(-3.0 * d as f32 / (rt60 * sample_rate as f32));
            comb.feedback = fb;
        }
    }
}

impl AudioEffect for Reverb {
    fn process(&mut self, samples: &mut [f32], _sample_rate: u32, channels: u16) {
        if channels != 2 {
            return;
        }
        let (pairs, _) = samples.as_chunks_mut::<2>();
        for pair in pairs {
            let input_l = pair[0];
            let input_r = pair[1];
            let mono = (input_l + input_r) * 0.5;

            let mut out = 0.0;
            for comb in &mut self.comb_delays {
                out += comb.process(mono);
            }
            out = self.allpass1.process(out);
            out = self.allpass2.process(out);

            pair[0] = input_l * self.dry + out * self.wet;
            pair[1] = input_r * self.dry + out * self.wet;
        }
    }

    fn reset(&mut self) {
        for comb in &mut self.comb_delays {
            comb.reset();
        }
        self.allpass1.reset();
        self.allpass2.reset();
    }

    fn set_parameter(&mut self, name: &str, value: f32) {
        match name {
            "decay" => self.decay = value.clamp(0.0, 1.0),
            "wet" => self.wet = value.clamp(0.0, 1.0),
            "dry" => self.dry = value.clamp(0.0, 1.0),
            _ => {}
        }
    }

    fn describe(&self) -> AudioEffectDescription {
        AudioEffectDescription {
            kind: "reverb",
            filter: None,
            params: vec![("decay", self.decay), ("wet", self.wet), ("dry", self.dry)],
        }
    }

    fn clone_box(&self) -> Box<dyn AudioEffect> {
        Box::new(self.clone())
    }
}

// ============================================================================
// Delay
// ============================================================================

#[derive(Clone, Debug)]
pub struct Delay {
    buffer: Vec<f32>,
    index: usize,
    pub delay_time: f32,
    pub feedback: f32,
    pub wet: f32,
    pub dry: f32,
    sample_rate: u32,
}

impl Delay {
    pub fn new(sample_rate: u32) -> Self {
        let delay_samples = (sample_rate as f32 * 0.3) as usize;
        Self {
            buffer: vec![0.0; delay_samples.max(1)],
            index: 0,
            delay_time: 0.3,
            feedback: 0.4,
            wet: 0.3,
            dry: 0.7,
            sample_rate,
        }
    }

    fn rebuild_buffer(&mut self) {
        let samples = (self.sample_rate as f32 * self.delay_time) as usize;
        let new_len = samples.max(1);
        if new_len != self.buffer.len() {
            self.buffer.resize(new_len, 0.0);
            if self.index >= new_len {
                self.index = 0;
            }
        }
    }
}

impl AudioEffect for Delay {
    fn process(&mut self, samples: &mut [f32], _sample_rate: u32, channels: u16) {
        if channels != 2 {
            return;
        }
        self.rebuild_buffer();
        let buf_len = self.buffer.len();
        let (pairs, _) = samples.as_chunks_mut::<2>();
        for pair in pairs {
            for ch in [0, 1] {
                let input = pair[ch];
                let delayed = self.buffer[self.index];
                self.buffer[self.index] = input + delayed * self.feedback;
                pair[ch] = input * self.dry + delayed * self.wet;
            }
            self.index = (self.index + 1) % buf_len;
        }
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.index = 0;
    }

    fn set_parameter(&mut self, name: &str, value: f32) {
        match name {
            "delay_time" | "time" => self.delay_time = value.max(0.001),
            "feedback" => self.feedback = value.clamp(0.0, 1.0),
            "wet" => self.wet = value.clamp(0.0, 1.0),
            "dry" => self.dry = value.clamp(0.0, 1.0),
            _ => {}
        }
    }

    fn describe(&self) -> AudioEffectDescription {
        AudioEffectDescription {
            kind: "delay",
            filter: None,
            params: vec![
                ("delay_time", self.delay_time),
                ("feedback", self.feedback),
                ("wet", self.wet),
                ("dry", self.dry),
            ],
        }
    }

    fn clone_box(&self) -> Box<dyn AudioEffect> {
        Box::new(self.clone())
    }
}

// ============================================================================
// Biquad EQ
// ============================================================================

#[derive(Clone, Debug, PartialEq)]
pub enum BiquadType {
    LowPass,
    HighPass,
    BandPass,
    Peak,
    Notch,
    LowShelf,
    HighShelf,
}

#[derive(Clone, Debug)]
pub struct BiquadEQ {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
    pub filter_type: BiquadType,
    pub frequency: f32,
    pub q: f32,
    pub gain_db: f32,
    sample_rate: u32,
    needs_recalc: bool,
}

impl BiquadEQ {
    pub fn new(sample_rate: u32) -> Self {
        let mut eq = Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
            filter_type: BiquadType::Peak,
            frequency: 1000.0,
            q: 0.707,
            gain_db: 0.0,
            sample_rate,
            needs_recalc: true,
        };
        eq.recalc();
        eq
    }

    fn recalc(&mut self) {
        let fs = self.sample_rate as f32;
        let w0 = 2.0 * consts::PI * self.frequency / fs;
        let cos_w0 = w0.cos();
        let sin_w0 = w0.sin();
        let alpha = sin_w0 / (2.0 * self.q);
        let a = 10.0_f32.powf(self.gain_db / 40.0);

        match self.filter_type {
            BiquadType::LowPass => {
                let b0 = (1.0 - cos_w0) / 2.0;
                let b1 = 1.0 - cos_w0;
                let b2 = (1.0 - cos_w0) / 2.0;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_w0;
                let a2 = 1.0 - alpha;
                self.b0 = b0 / a0;
                self.b1 = b1 / a0;
                self.b2 = b2 / a0;
                self.a1 = a1 / a0;
                self.a2 = a2 / a0;
            }
            BiquadType::HighPass => {
                let b0 = (1.0 + cos_w0) / 2.0;
                let b1 = -(1.0 + cos_w0);
                let b2 = (1.0 + cos_w0) / 2.0;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_w0;
                let a2 = 1.0 - alpha;
                self.b0 = b0 / a0;
                self.b1 = b1 / a0;
                self.b2 = b2 / a0;
                self.a1 = a1 / a0;
                self.a2 = a2 / a0;
            }
            BiquadType::BandPass => {
                let b0 = alpha;
                let b1 = 0.0;
                let b2 = -alpha;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_w0;
                let a2 = 1.0 - alpha;
                self.b0 = b0 / a0;
                self.b1 = b1 / a0;
                self.b2 = b2 / a0;
                self.a1 = a1 / a0;
                self.a2 = a2 / a0;
            }
            BiquadType::Notch => {
                let b0 = 1.0;
                let b1 = -2.0 * cos_w0;
                let b2 = 1.0;
                let a0 = 1.0 + alpha;
                let a1 = -2.0 * cos_w0;
                let a2 = 1.0 - alpha;
                self.b0 = b0 / a0;
                self.b1 = b1 / a0;
                self.b2 = b2 / a0;
                self.a1 = a1 / a0;
                self.a2 = a2 / a0;
            }
            BiquadType::Peak => {
                let b0 = 1.0 + alpha * a;
                let b1 = -2.0 * cos_w0;
                let b2 = 1.0 - alpha * a;
                let a0 = 1.0 + alpha / a;
                let a1 = -2.0 * cos_w0;
                let a2 = 1.0 - alpha / a;
                self.b0 = b0 / a0;
                self.b1 = b1 / a0;
                self.b2 = b2 / a0;
                self.a1 = a1 / a0;
                self.a2 = a2 / a0;
            }
            BiquadType::LowShelf => {
                let sqrt_a = a.sqrt();
                let b0 = a * ((a + 1.0) - (a - 1.0) * cos_w0 + 2.0 * sqrt_a * alpha);
                let b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w0);
                let b2 = a * ((a + 1.0) - (a - 1.0) * cos_w0 - 2.0 * sqrt_a * alpha);
                let a0 = (a + 1.0) + (a - 1.0) * cos_w0 + 2.0 * sqrt_a * alpha;
                let a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cos_w0);
                let a2 = (a + 1.0) + (a - 1.0) * cos_w0 - 2.0 * sqrt_a * alpha;
                self.b0 = b0 / a0;
                self.b1 = b1 / a0;
                self.b2 = b2 / a0;
                self.a1 = a1 / a0;
                self.a2 = a2 / a0;
            }
            BiquadType::HighShelf => {
                let sqrt_a = a.sqrt();
                let b0 = a * ((a + 1.0) + (a - 1.0) * cos_w0 + 2.0 * sqrt_a * alpha);
                let b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0);
                let b2 = a * ((a + 1.0) + (a - 1.0) * cos_w0 - 2.0 * sqrt_a * alpha);
                let a0 = (a + 1.0) - (a - 1.0) * cos_w0 + 2.0 * sqrt_a * alpha;
                let a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos_w0);
                let a2 = (a + 1.0) - (a - 1.0) * cos_w0 - 2.0 * sqrt_a * alpha;
                self.b0 = b0 / a0;
                self.b1 = b1 / a0;
                self.b2 = b2 / a0;
                self.a1 = a1 / a0;
                self.a2 = a2 / a0;
            }
        }
        self.needs_recalc = false;
    }

    fn process_channel(&mut self, input: f32) -> f32 {
        if self.needs_recalc {
            self.recalc();
        }
        let output = self.b0 * input + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;
        self.x2 = self.x1;
        self.x1 = input;
        self.y2 = self.y1;
        self.y1 = output;
        output
    }
}

impl AudioEffect for BiquadEQ {
    fn process(&mut self, samples: &mut [f32], _sample_rate: u32, channels: u16) {
        if channels == 2 {
            let (pairs, _) = samples.as_chunks_mut::<2>();
            for pair in pairs {
                pair[0] = self.process_channel(pair[0]);
                pair[1] = self.process_channel(pair[1]);
            }
        } else {
            for s in samples.iter_mut() {
                *s = self.process_channel(*s);
            }
        }
    }

    fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }

    fn set_parameter(&mut self, name: &str, value: f32) {
        match name {
            "frequency" | "freq" => {
                self.frequency = value.clamp(10.0, 20000.0);
                self.needs_recalc = true;
            }
            "q" | "resonance" => {
                self.q = value.max(0.01);
                self.needs_recalc = true;
            }
            "gain" | "gain_db" => {
                self.gain_db = value.clamp(-24.0, 24.0);
                self.needs_recalc = true;
            }
            _ => {}
        }
    }

    fn describe(&self) -> AudioEffectDescription {
        AudioEffectDescription {
            kind: "eq",
            filter: Some(match self.filter_type {
                BiquadType::LowPass => "low_pass",
                BiquadType::HighPass => "high_pass",
                BiquadType::BandPass => "band_pass",
                BiquadType::Peak => "peak",
                BiquadType::Notch => "notch",
                BiquadType::LowShelf => "low_shelf",
                BiquadType::HighShelf => "high_shelf",
            }),
            params: vec![("frequency", self.frequency), ("q", self.q), ("gain_db", self.gain_db)],
        }
    }

    fn clone_box(&self) -> Box<dyn AudioEffect> {
        Box::new(self.clone())
    }
}

// ============================================================================
// Compressor
// ============================================================================

#[derive(Clone, Debug)]
pub struct Compressor {
    pub threshold: f32,
    pub ratio: f32,
    pub knee: f32,
    pub attack: f32,
    pub release: f32,
    pub makeup_gain: f32,
    envelope: f32,
    sample_rate: u32,
}

impl Compressor {
    pub fn new(sample_rate: u32) -> Self {
        Self {
            threshold: -18.0,
            ratio: 4.0,
            knee: 6.0,
            attack: 0.003,
            release: 0.15,
            makeup_gain: 6.0,
            envelope: 0.0,
            sample_rate,
        }
    }

    fn process_channel(&mut self, input: f32) -> f32 {
        let input_db = 20.0 * input.abs().log10().max(-100.0);

        let attack_coeff = (-1.0 / (self.sample_rate as f32 * self.attack)).exp();
        let release_coeff = (-1.0 / (self.sample_rate as f32 * self.release)).exp();

        let env_db = if input_db > self.envelope {
            self.envelope + (1.0 - attack_coeff) * (input_db - self.envelope)
        } else {
            self.envelope + (1.0 - release_coeff) * (input_db - self.envelope)
        };
        self.envelope = env_db;

        let knee_low = self.threshold - self.knee / 2.0;
        let knee_high = self.threshold + self.knee / 2.0;

        let gain_db = if env_db < knee_low {
            0.0
        } else if env_db < knee_high {
            let x = (env_db - knee_low) / self.knee;
            let x_sq = x * x;
            x_sq * (1.0 / self.ratio - 1.0) * (knee_high - env_db) / (2.0 * self.knee)
                + x_sq * (env_db - self.threshold) * (1.0 / self.ratio - 1.0) / self.knee
        } else {
            (self.threshold - env_db) * (1.0 - 1.0 / self.ratio)
        };

        let gain_linear = 10.0_f32.powf((gain_db + self.makeup_gain) / 20.0);
        input * gain_linear
    }
}

impl AudioEffect for Compressor {
    fn process(&mut self, samples: &mut [f32], _sample_rate: u32, channels: u16) {
        if channels == 2 {
            let (pairs, _) = samples.as_chunks_mut::<2>();
            for pair in pairs {
                pair[0] = self.process_channel(pair[0]);
                pair[1] = self.process_channel(pair[1]);
            }
        } else {
            for s in samples.iter_mut() {
                *s = self.process_channel(*s);
            }
        }
    }

    fn reset(&mut self) {
        self.envelope = 0.0;
    }

    fn set_parameter(&mut self, name: &str, value: f32) {
        match name {
            "threshold" => self.threshold = value,
            "ratio" => self.ratio = value.max(1.0),
            "knee" => self.knee = value.max(0.0),
            "attack" => self.attack = value.max(0.0001),
            "release" => self.release = value.max(0.001),
            "makeup_gain" | "makeup" => self.makeup_gain = value,
            _ => {}
        }
    }

    fn describe(&self) -> AudioEffectDescription {
        AudioEffectDescription {
            kind: "compressor",
            filter: None,
            params: vec![
                ("threshold", self.threshold),
                ("ratio", self.ratio),
                ("knee", self.knee),
                ("attack", self.attack),
                ("release", self.release),
                ("makeup_gain", self.makeup_gain),
            ],
        }
    }

    fn clone_box(&self) -> Box<dyn AudioEffect> {
        Box::new(self.clone())
    }
}

// ============================================================================
// Effect Parameter Animation
// ============================================================================

#[derive(Clone, Debug)]
pub struct EffectParamAnimation {
    pub effect_idx: usize,
    pub param_name: String,
    pub animated: crate::animation::Animated<f32>,
}
