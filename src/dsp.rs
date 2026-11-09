//! Sound processing applied to every song: equalizer, mono, fades for crossfade
//! and automix, and silence detection.
//!
//! Each song gets a [`Processor`] wrapped around its decoder. The processor reads
//! the shared [`Effects`] (changed live from Settings) and its own [`TrackCtl`]
//! (fade commands from the engine, and how much silence it is hearing).

use crate::settings::{EQ_BANDS, Settings};
use rodio::source::SeekError;
use rodio::{ChannelCount, SampleRate, Source};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Settings that change the sound. Copied from [`Settings`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Effects {
    pub eq: Option<[f32; 6]>,
    pub mono: bool,
    /// Seconds of crossfade (0 = none).
    pub crossfade: f32,
    pub gapless: bool,
    pub automix: bool,
}

impl Effects {
    pub fn from_settings(s: &Settings) -> Self {
        Self {
            eq: s.eq_enabled.then_some(s.eq_gains).filter(|g| g.iter().any(|v| v.abs() > 0.01)),
            mono: s.mono,
            crossfade: s.crossfade_secs.clamp(0.0, 10.0),
            gapless: s.gapless,
            automix: s.automix,
        }
    }

    /// Length of the overlap between songs, in seconds.
    pub fn transition_secs(&self) -> f32 {
        if self.automix { self.crossfade.max(6.0) } else { self.crossfade }
    }
}

/// Effects shared by all playing songs. `version` changes whenever they do.
#[derive(Default)]
pub struct SharedEffects {
    effects: Mutex<Effects>,
    version: AtomicU64,
}

impl SharedEffects {
    pub fn set(&self, e: Effects) {
        let mut cur = self.effects.lock().unwrap();
        if *cur != e {
            *cur = e;
            self.version.fetch_add(1, Ordering::Release);
        }
    }

    pub fn get(&self) -> Effects {
        self.effects.lock().unwrap().clone()
    }
}

/// Engine → song: fade commands. Song → engine: silence and progress.
#[derive(Default)]
pub struct TrackCtl {
    fade: Mutex<Option<(f32, f32, Duration)>>,
    fade_version: AtomicU64,
    /// How long the song has been silent right now, in milliseconds.
    silent_ms: AtomicU64,
    /// The fade set last has finished.
    fade_done: AtomicBool,
}

impl TrackCtl {
    /// Fades the song's volume from `from` to `to` over `over`.
    pub fn fade(&self, from: f32, to: f32, over: Duration) {
        *self.fade.lock().unwrap() = Some((from, to, over));
        self.fade_done.store(false, Ordering::Release);
        self.fade_version.fetch_add(1, Ordering::Release);
    }

    pub fn silent_for(&self) -> Duration {
        Duration::from_millis(self.silent_ms.load(Ordering::Acquire))
    }

    pub fn fade_finished(&self) -> bool {
        self.fade_done.load(Ordering::Acquire)
    }
}

/// Below this level (about -50 dB) a frame counts as silence.
const SILENCE: f32 = 0.003;
/// How often (in frames) the processor checks for new settings and fade commands.
const BLOCK: u64 = 256;
/// Automix skips at most this much silence at the start of a song.
const MAX_LEADING_SILENCE: Duration = Duration::from_secs(4);

// ---------------------------------------------------------------------------
// Equalizer: one biquad filter per band (RBJ Audio EQ Cookbook).
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    /// A peaking filter: `gain_db` at `freq`, fading back to 0 dB around it.
    fn peak(freq: f32, gain_db: f32, rate: f32) -> Self {
        let freq = freq.min(rate * 0.45);
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f32::consts::PI * freq / rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * 1.0); // Q = 1
        let (b0, b1, b2) = (1.0 + alpha * a, -2.0 * cos, 1.0 - alpha * a);
        let (a0, a1, a2) = (1.0 + alpha / a, -2.0 * cos, 1.0 - alpha / a);
        Self { b0: b0 / a0, b1: b1 / a0, b2: b2 / a0, a1: a1 / a0, a2: a2 / a0, z1: 0.0, z2: 0.0 }
    }

    /// Changes the filter's settings but keeps its state, so there's no click.
    fn retune(&mut self, other: Biquad) {
        let (z1, z2) = (self.z1, self.z2);
        *self = other;
        self.z1 = z1;
        self.z2 = z2;
    }

    #[inline]
    fn run(&mut self, x: f32) -> f32 {
        // Transposed direct form II.
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

fn eq_bank(gains: &[f32; 6], rate: f32) -> [Biquad; 6] {
    std::array::from_fn(|i| {
        // Peaks everywhere, so each point's gain is exactly what plays at its frequency.
        Biquad::peak(EQ_BANDS[i], gains[i], rate)
    })
}

/// Lowers the volume when bands are boosted, so loud songs don't clip.
fn preamp(gains: &[f32; 6]) -> f32 {
    let boost = gains.iter().cloned().fold(0.0f32, f32::max);
    10f32.powf(-boost * 0.5 / 20.0)
}

// ---------------------------------------------------------------------------
// The processor
// ---------------------------------------------------------------------------

pub struct Processor<S: Source> {
    inner: S,
    channels: ChannelCount,
    rate: SampleRate,
    shared: Arc<SharedEffects>,
    seen_version: u64,
    effects: Effects,
    eq: Vec<[Biquad; 6]>,
    eq_gain: f32,
    ctl: Arc<TrackCtl>,
    seen_fade: u64,
    /// Current fade: gain now, gain per frame, frames left, target.
    gain: f32,
    gain_step: f32,
    fade_left: u64,
    fade_to: f32,
    frame: Vec<f32>,
    frame_pos: usize,
    frames: u64,
    silent_frames: u64,
    skip_leading_silence: bool,
}

impl<S: Source> Processor<S> {
    /// `start_gain` is the starting volume (0 for a song that fades in).
    pub fn new(inner: S, shared: Arc<SharedEffects>, ctl: Arc<TrackCtl>, start_gain: f32, skip_leading_silence: bool) -> Self {
        let channels = inner.channels();
        let rate = inner.sample_rate();
        let mut p = Self {
            inner,
            channels,
            rate,
            shared,
            seen_version: u64::MAX,
            effects: Effects::default(),
            eq: vec![],
            eq_gain: 1.0,
            ctl,
            seen_fade: 0,
            gain: start_gain,
            gain_step: 0.0,
            fade_left: 0,
            fade_to: start_gain,
            frame: Vec::with_capacity(channels.get() as usize),
            frame_pos: 0,
            frames: 0,
            silent_frames: 0,
            skip_leading_silence,
        };
        p.refresh();
        p
    }

    /// Picks up changed settings and fade commands (once per block of frames).
    fn refresh(&mut self) {
        let v = self.shared.version.load(Ordering::Acquire);
        if v != self.seen_version {
            self.seen_version = v;
            self.effects = self.shared.get();
            let rate = self.rate.get() as f32;
            let ch = self.channels.get() as usize;
            match self.effects.eq {
                Some(g) => {
                    let bank = eq_bank(&g, rate);
                    if self.eq.len() != ch {
                        self.eq = vec![bank; ch];
                    } else {
                        for filters in &mut self.eq {
                            for (f, n) in filters.iter_mut().zip(bank) {
                                f.retune(n);
                            }
                        }
                    }
                    self.eq_gain = preamp(&g);
                }
                None => {
                    self.eq.clear();
                    self.eq_gain = 1.0;
                }
            }
        }
        let fv = self.ctl.fade_version.load(Ordering::Acquire);
        if fv != self.seen_fade {
            self.seen_fade = fv;
            if let Some((from, to, over)) = *self.ctl.fade.lock().unwrap() {
                let frames = (over.as_secs_f32() * self.rate.get() as f32).max(1.0) as u64;
                // Start from the current level if it's mid-fade, so there's no jump.
                self.gain = if self.fade_left > 0 { self.gain } else { from };
                self.fade_to = to;
                self.fade_left = frames;
                self.gain_step = (to - self.gain) / frames as f32;
            }
        }
        self.ctl.silent_ms.store(self.silent_frames * 1000 / self.rate.get() as u64, Ordering::Release);
    }

    /// Reads and processes the next frame (one sample per channel). False at the end.
    fn next_frame(&mut self) -> bool {
        let ch = self.channels.get() as usize;
        loop {
            self.frame.clear();
            for _ in 0..ch {
                match self.inner.next() {
                    Some(s) => self.frame.push(s),
                    None => break,
                }
            }
            if self.frame.len() < ch {
                // End of song (or a broken last frame): pass through what's left.
                return !self.frame.is_empty();
            }
            let peak = self.frame.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            if peak < SILENCE {
                self.silent_frames += 1;
            } else {
                self.silent_frames = 0;
                self.skip_leading_silence = false;
            }
            // Automix: skip silence at the very start of the song.
            if self.skip_leading_silence
                && self.silent_frames > 0
                && self.frames < MAX_LEADING_SILENCE.as_secs() * self.rate.get() as u64
            {
                self.frames += 1;
                continue;
            }
            break;
        }
        self.frames += 1;
        if self.frames.is_multiple_of(BLOCK) {
            self.refresh();
        }

        if self.effects.mono && ch > 1 {
            let avg = self.frame.iter().sum::<f32>() / ch as f32;
            self.frame.iter_mut().for_each(|s| *s = avg);
        }
        if !self.eq.is_empty() {
            for (s, filters) in self.frame.iter_mut().zip(self.eq.iter_mut()) {
                let mut x = *s * self.eq_gain;
                for f in filters.iter_mut() {
                    x = f.run(x);
                }
                *s = x.clamp(-1.0, 1.0);
            }
        }
        if self.fade_left > 0 {
            self.gain += self.gain_step;
            self.fade_left -= 1;
            if self.fade_left == 0 {
                self.gain = self.fade_to;
                self.ctl.fade_done.store(true, Ordering::Release);
            }
        }
        if self.gain != 1.0 {
            let g = self.gain.clamp(0.0, 1.0);
            self.frame.iter_mut().for_each(|s| *s *= g);
        }
        true
    }
}

impl<S: Source> Iterator for Processor<S> {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if self.frame_pos >= self.frame.len() {
            if !self.next_frame() {
                return None;
            }
            self.frame_pos = 0;
        }
        let s = self.frame[self.frame_pos];
        self.frame_pos += 1;
        Some(s)
    }
}

impl<S: Source> Source for Processor<S> {
    fn current_span_len(&self) -> Option<usize> {
        // Audio is processed in whole frames, so report "until the end" instead of
        // the decoder's span boundaries.
        None
    }

    fn channels(&self) -> ChannelCount {
        self.channels
    }

    fn sample_rate(&self) -> SampleRate {
        self.rate
    }

    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }

    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.frame.clear();
        self.frame_pos = 0;
        self.silent_frames = 0;
        self.skip_leading_silence = false;
        self.frames = (pos.as_secs_f64() * self.rate.get() as f64) as u64;
        self.inner.try_seek(pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::source::SineWave;

    fn rms(v: &[f32]) -> f32 {
        (v.iter().map(|s| s * s).sum::<f32>() / v.len() as f32).sqrt()
    }

    fn run(freq: f32, effects: Effects) -> Vec<f32> {
        let shared = Arc::new(SharedEffects::default());
        shared.set(effects);
        let src = SineWave::new(freq).amplify(0.3).take_duration(Duration::from_millis(500));
        let p = Processor::new(src, shared, Arc::new(TrackCtl::default()), 1.0, false);
        p.skip(4000).collect()
    }

    #[test]
    fn flat_settings_leave_the_sound_alone() {
        let a = run(440.0, Effects::default());
        assert!((rms(&a) - 0.3 / 2f32.sqrt()).abs() < 0.01);
    }

    #[test]
    fn eq_boosts_and_cuts_the_right_band() {
        let flat = rms(&run(60.0, Effects::default()));
        let boost = Effects { eq: Some([12.0, 0.0, 0.0, 0.0, 0.0, 0.0]), ..Default::default() };
        // Preamp lowers everything by half the boost (6 dB); the 60 Hz band still ends up clearly louder.
        assert!(rms(&run(60.0, boost.clone())) > flat * 1.5);
        // A 5 kHz tone is barely touched by the bass band (only the preamp applies).
        let hi_flat = rms(&run(5000.0, Effects::default()));
        let hi = rms(&run(5000.0, boost));
        assert!(hi < hi_flat && hi > hi_flat * 0.3);
        let cut = Effects { eq: Some([0.0, 0.0, 0.0, -12.0, 0.0, 0.0]), ..Default::default() };
        assert!(rms(&run(1000.0, cut)) < rms(&run(1000.0, Effects::default())) * 0.5);
    }

    #[test]
    fn fades_reach_their_target() {
        let shared = Arc::new(SharedEffects::default());
        let ctl = Arc::new(TrackCtl::default());
        let src = SineWave::new(440.0).take_duration(Duration::from_secs(2));
        let mut p = Processor::new(src, shared, ctl.clone(), 1.0, false);
        ctl.fade(1.0, 0.0, Duration::from_millis(100));
        let out: Vec<f32> = p.by_ref().take(48000).collect();
        assert!(ctl.fade_finished());
        assert!(rms(&out[40000..]) < 1e-6, "silent after fading out");
    }

    #[test]
    fn notices_silence() {
        let shared = Arc::new(SharedEffects::default());
        let ctl = Arc::new(TrackCtl::default());
        let src = SineWave::new(440.0).amplify(0.0).take_duration(Duration::from_secs(2));
        let p = Processor::new(src, shared, ctl.clone(), 1.0, false);
        let _ = p.take(48000).count();
        assert!(ctl.silent_for() >= Duration::from_millis(900));
    }

    #[test]
    fn mono_makes_both_sides_equal() {
        let shared = Arc::new(SharedEffects::default());
        shared.set(Effects { mono: true, ..Default::default() });
        let left = SineWave::new(440.0).take_duration(Duration::from_millis(100));
        let stereo = rodio::source::ChannelVolume::new(left, vec![1.0, 0.0]);
        let out: Vec<f32> = Processor::new(stereo, shared, Arc::new(TrackCtl::default()), 1.0, false).collect();
        assert!(out.chunks(2).all(|f| (f[0] - f[1]).abs() < 1e-6));
        assert!(rms(&out) > 0.1);
    }

    #[test]
    fn automix_skips_silence_at_the_start() {
        let shared = Arc::new(SharedEffects::default());
        // One second of silence, then a tone.
        let mut samples = vec![0.0f32; 48000];
        samples.extend((0..48000).map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / 48000.0).sin()));
        let one = std::num::NonZero::new(1u16).unwrap();
        let rate = std::num::NonZero::new(48000u32).unwrap();
        let src = rodio::buffer::SamplesBuffer::new(one, rate, samples);
        let out: Vec<f32> = Processor::new(src, shared, Arc::new(TrackCtl::default()), 1.0, true).take(4800).collect();
        assert!(rms(&out) > 0.5, "starts straight with the music");
    }
}
