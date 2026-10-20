//! Fixed-capacity mixer. No ECS work, file access or sample-buffer copies
//! occur in the device callback. Commands are consumed once per 256-frame block.
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc::Receiver,
    Arc,
};

pub const VOICES: usize = 64;
pub const RATE: u32 = 48_000;
pub const BLOCK: usize = 256;

#[derive(Debug)]
pub struct Pcm {
    pub data: Box<[i16]>,
    pub channels: usize,
    pub rate: u32,
    pub loop_frames: Option<(usize, usize)>,
}
impl Pcm {
    pub fn frames(&self) -> usize {
        self.data.len() / self.channels
    }
    pub fn wav(bytes: &[u8]) -> Result<Self, std::io::Error> {
        let bad = || {
            std::io::Error::other("expected a complete mono/stereo PCM16 WAV; run export_audio.py")
        };
        let u16_at = |at| {
            bytes
                .get(at..at + 2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
        };
        let u32_at = |at| {
            bytes
                .get(at..at + 4)
                .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        };
        if bytes.get(..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
            return Err(bad());
        }
        let end = u32_at(4).ok_or_else(bad)? as usize + 8;
        if end > bytes.len() {
            return Err(bad());
        }
        let (mut format, mut data, mut loop_frames) = (None, None, None);
        let mut at = 12;
        while at + 8 <= end {
            let n = u32_at(at + 4).ok_or_else(bad)? as usize;
            let p = at + 8;
            if p + n > end {
                return Err(bad());
            }
            match &bytes[at..at + 4] {
                b"fmt " if n >= 16 => {
                    format = Some((
                        u16_at(p).unwrap(),
                        u16_at(p + 2).unwrap(),
                        u32_at(p + 4).unwrap(),
                        u16_at(p + 14).unwrap(),
                    ));
                }
                b"data" => {
                    data = Some(&bytes[p..p + n]);
                }
                b"smpl" if n >= 60 && u32_at(p + 28).unwrap() > 0 => {
                    loop_frames = Some((
                        u32_at(p + 44).unwrap() as usize,
                        u32_at(p + 48).unwrap() as usize + 1,
                    ));
                }
                _ => {}
            }
            at = p + n + (n & 1);
        }
        let (encoding, channels, rate, bits) = format.ok_or_else(bad)?;
        let data = data.ok_or_else(bad)?;
        if encoding != 1
            || !(1..=2).contains(&channels)
            || !(8000..=192000).contains(&rate)
            || bits != 16
            || data.is_empty()
            || data.len() % (2 * channels as usize) != 0
        {
            return Err(bad());
        }
        let frames = data.len() / (2 * channels as usize);
        Ok(Self {
            channels: channels as usize,
            rate,
            data: data
                .chunks_exact(2)
                .map(|b| i16::from_le_bytes([b[0], b[1]]))
                .collect(),
            loop_frames: loop_frames.filter(|&(s, e)| s < e && e <= frames),
        })
    }
}

#[derive(Clone, Copy, Default)]
pub struct Control {
    pub token: u64,
    pub gain: [f32; 2],
    pub stop: bool,
    pub fade: f32,
}
pub enum Command {
    Start {
        slot: usize,
        token: u64,
        pcm: Arc<Pcm>,
        looping: bool,
        spatial: bool,
        gain: [f32; 2],
        fade: f32,
    },
    Controls(Box<[Control; VOICES]>),
}
struct Voice {
    pcm: Arc<Pcm>,
    token: u64,
    position: f64,
    looping: bool,
    spatial: bool,
    gain: [f32; 2],
    target: [f32; 2],
    envelope: f32,
    attack: f32,
    release: Option<f32>,
}
pub struct Mixer {
    receiver: Receiver<Command>,
    finished: Arc<[AtomicU64; VOICES]>,
    voices: [Option<Voice>; VOICES],
    block: [f32; BLOCK * 2],
    cursor: usize,
    limiter: f32,
}
impl Mixer {
    pub fn new(receiver: Receiver<Command>, finished: Arc<[AtomicU64; VOICES]>) -> Self {
        Self {
            receiver,
            finished,
            voices: std::array::from_fn(|_| None),
            block: [0.; BLOCK * 2],
            cursor: BLOCK * 2,
            limiter: 1.,
        }
    }
    fn render(&mut self) {
        // Bounded queue prevents an effect storm from monopolizing the callback.
        for _ in 0..128 {
            let Ok(command) = self.receiver.try_recv() else {
                break;
            };
            match command {
                Command::Start {
                    slot,
                    token,
                    pcm,
                    looping,
                    spatial,
                    gain,
                    fade,
                } => {
                    if slot >= VOICES {
                        continue;
                    }
                    self.voices[slot] = Some(Voice {
                        pcm,
                        token,
                        position: 0.,
                        looping,
                        spatial,
                        gain,
                        target: gain,
                        envelope: 0.,
                        attack: 1. / (fade.max(0.005) * RATE as f32),
                        release: None,
                    });
                }
                Command::Controls(controls) => {
                    for (voice, control) in self.voices.iter_mut().zip(controls.iter()) {
                        if let Some(v) = voice.as_mut().filter(|v| v.token == control.token) {
                            v.target = control.gain;
                            if control.stop && v.release.is_none() {
                                v.release =
                                    Some(v.envelope / (RATE as f32 * control.fade.max(0.005)));
                            }
                        }
                    }
                }
            }
        }
        self.block.fill(0.);
        for (slot, entry) in self.voices.iter_mut().enumerate() {
            let Some(v) = entry else { continue };
            let frames = v.pcm.frames();
            let (loop_start, loop_end) = v.pcm.loop_frames.unwrap_or((0, frames));
            let step = v.pcm.rate as f64 / RATE as f64;
            let mut ended = false;
            for out in self.block.chunks_exact_mut(2) {
                if v.looping && v.position >= loop_end as f64 {
                    v.position = loop_start as f64
                        + (v.position - loop_start as f64) % (loop_end - loop_start) as f64;
                }
                if v.position >= frames as f64 {
                    ended = true;
                    break;
                }
                if let Some(release) = v.release {
                    v.envelope = (v.envelope - release.max(1. / (RATE as f32 * 10.))).max(0.);
                    if v.envelope == 0. {
                        ended = true;
                        break;
                    }
                } else {
                    v.envelope = (v.envelope + v.attack).min(1.);
                }
                let a = v.position as usize;
                let b = if v.looping && a + 1 >= loop_end {
                    loop_start
                } else {
                    (a + 1).min(frames - 1)
                };
                let fraction = (v.position - a as f64) as f32;
                for (channel, output) in out.iter_mut().enumerate() {
                    v.gain[channel] += (v.target[channel] - v.gain[channel]) * 0.002;
                    let c = channel.min(v.pcm.channels - 1);
                    let sample = |frame: usize| {
                        if v.spatial && v.pcm.channels == 2 {
                            (v.pcm.data[frame * 2] as f32 + v.pcm.data[frame * 2 + 1] as f32)
                                / 65536.
                        } else {
                            v.pcm.data[frame * v.pcm.channels + c] as f32 / 32768.
                        }
                    };
                    let x = sample(a);
                    let y = sample(b);
                    // Short end ramp also prevents clicks on non-zero-ended one-shots.
                    let end_gain = if v.looping {
                        1.
                    } else {
                        ((frames as f64 - v.position) / (step * 240.)).min(1.) as f32
                    };
                    *output += (x + (y - x) * fraction) * v.gain[channel] * v.envelope * end_gain;
                }
                v.position += step;
            }
            if ended {
                self.finished[slot].store(v.token, Ordering::Release);
                *entry = None;
            }
        }
        // Block lookahead: instantaneous attack, gradual release; preserve stereo
        // balance and avoid digital clipping when many spells coincide.
        let peak = self.block.iter().fold(1_f32, |p, x| p.max(x.abs()));
        self.limiter = (self.limiter + 0.002).min(0.95 / peak);
        for value in &mut self.block {
            *value *= self.limiter;
        }
        self.cursor = 0;
    }
}
impl Iterator for Mixer {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.cursor == self.block.len() {
            self.render();
        }
        let sample = self.block[self.cursor];
        self.cursor += 1;
        Some(sample)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::sync_channel;
    fn pcm(frames: usize) -> Arc<Pcm> {
        Arc::new(Pcm {
            data: vec![24000; frames].into(),
            channels: 1,
            rate: RATE,
            loop_frames: None,
        })
    }
    #[test]
    fn shared_voices_finish_and_slots_reuse() {
        let (tx, rx) = sync_channel(128);
        let done = Arc::new(std::array::from_fn(|_| AtomicU64::new(0)));
        let mut mix = Mixer::new(rx, done.clone());
        let sample = pcm(480);
        for generation in 1..=20 {
            for slot in 0..VOICES {
                tx.send(Command::Start {
                    slot,
                    token: generation,
                    pcm: sample.clone(),
                    looping: false,
                    spatial: false,
                    gain: [1.; 2],
                    fade: 0.005,
                })
                .unwrap();
            }
            assert!(mix
                .by_ref()
                .take(2048)
                .all(|v| v.is_finite() && v.abs() <= 0.951));
            assert!(done.iter().all(|v| v.load(Ordering::Acquire) == generation));
            assert_eq!(Arc::strong_count(&sample), 1);
        }
    }
    #[test]
    fn loops_stop_and_stale_controls_do_not_stop_reused_voice() {
        let (tx, rx) = sync_channel(128);
        let done = Arc::new(std::array::from_fn(|_| AtomicU64::new(0)));
        let mut mix = Mixer::new(rx, done.clone());
        tx.send(Command::Start {
            slot: 0,
            token: 2,
            pcm: pcm(10),
            looping: true,
            spatial: false,
            gain: [1.; 2],
            fade: 0.005,
        })
        .unwrap();
        let mut control = Box::new([Control::default(); VOICES]);
        control[0] = Control {
            token: 1,
            stop: true,
            gain: [0.; 2],
            fade: 0.15,
        };
        tx.send(Command::Controls(control.clone())).unwrap();
        assert!(mix.by_ref().take(5000).any(|v| v > 0.));
        assert_eq!(done[0].load(Ordering::Acquire), 0);
        control[0].token = 2;
        tx.send(Command::Controls(control)).unwrap();
        mix.by_ref().take(20000).for_each(drop);
        assert_eq!(done[0].load(Ordering::Acquire), 2);
    }
    #[test]
    fn rejects_compressed_truncated_and_empty_wavs() {
        assert!(Pcm::wav(b"RIFF\xff\xff\xff\xffWAVE").is_err());
        assert!(Pcm::wav(b"RIFF\x04\0\0\0WAVE").is_err());
        assert!(Pcm::wav(b"FSB5compressed").is_err());
    }
    #[test]
    fn wav_reader_preserves_channels_rate_and_embedded_loop_points() {
        fn chunk(out: &mut Vec<u8>, tag: &[u8; 4], data: &[u8]) {
            out.extend(tag);
            out.extend((data.len() as u32).to_le_bytes());
            out.extend(data);
            if data.len() % 2 != 0 {
                out.push(0);
            }
        }
        let mut wave = b"RIFF\0\0\0\0WAVE".to_vec();
        chunk(&mut wave, b"JUNK", b"odd");
        let mut format = Vec::new();
        format.extend(1u16.to_le_bytes());
        format.extend(2u16.to_le_bytes());
        format.extend(22050u32.to_le_bytes());
        format.extend(88200u32.to_le_bytes());
        format.extend(4u16.to_le_bytes());
        format.extend(16u16.to_le_bytes());
        chunk(&mut wave, b"fmt ", &format);
        let mut loop_data = vec![0; 60];
        loop_data[28..32].copy_from_slice(&1u32.to_le_bytes());
        loop_data[44..48].copy_from_slice(&1u32.to_le_bytes());
        loop_data[48..52].copy_from_slice(&2u32.to_le_bytes());
        chunk(&mut wave, b"smpl", &loop_data);
        let data: Vec<u8> = [100i16, -100, 200, -200, 300, -300, 400, -400]
            .into_iter()
            .flat_map(i16::to_le_bytes)
            .collect();
        chunk(&mut wave, b"data", &data);
        let length = (wave.len() - 8) as u32;
        wave[4..8].copy_from_slice(&length.to_le_bytes());
        let pcm = Pcm::wav(&wave).unwrap();
        assert_eq!((pcm.channels, pcm.rate, pcm.frames()), (2, 22050, 4));
        assert_eq!(pcm.loop_frames, Some((1, 3)));
        assert_eq!(&*pcm.data, &[100, -100, 200, -200, 300, -300, 400, -400]);
    }
}
