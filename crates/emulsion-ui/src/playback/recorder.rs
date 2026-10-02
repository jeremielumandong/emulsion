//! Microphone recording (T4) from a chosen input device (T13). The
//! device's stream lives on its own thread, which hands each buffer to a
//! [`Capture`] that writes a 16-bit WAV (mono for a mono microphone, else
//! the first two channels) and keeps the peak level for a meter. Everything
//! but [`Recording::start`] and [`input_devices`] is plain data, so tests
//! drive a capture by hand and never need a device.
use emulsion_io::audio::wav::{SampleFormat, WavWriter};
use parking_lot::Mutex;
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

/// The loudest sample (absolute) of interleaved `data`.
pub(crate) fn peak(data: &[f32]) -> f32 {
    data.iter().fold(0f32, |m, s| m.max(s.abs()))
}

/// A level meter's reading: the loudest sample since it was last read.
#[derive(Default)]
pub(crate) struct Meter(AtomicU32);

impl Meter {
    pub fn feed(&self, level: f32) {
        self.0
            .fetch_max(level.clamp(0., 1.).to_bits(), Ordering::Relaxed);
    }
    /// The peak since the last call, 0–1.
    pub fn take(&self) -> f32 {
        f32::from_bits(self.0.swap(0, Ordering::Relaxed))
    }
}

/// Writes device buffers to a WAV file.
pub(crate) struct Capture {
    writer: WavWriter<BufWriter<File>>,
    rate: u32,
    /// Channels the device delivers, and the file keeps (at most two).
    input: usize,
    output: usize,
    scratch: Vec<f32>,
}

impl Capture {
    pub fn new(path: &Path, rate: u32, channels: u16) -> Result<Self, String> {
        let input = usize::from(channels.max(1));
        let output = input.min(2);
        let writer = WavWriter::create(path, rate, output as u16, SampleFormat::Int16)
            .map_err(|e| format!("Cannot write the recording: {e}"))?;
        Ok(Self {
            writer,
            rate,
            input,
            output,
            scratch: Vec::new(),
        })
    }

    /// Write one interleaved device buffer.
    pub fn push(&mut self, data: &[f32]) -> Result<(), String> {
        let write = if self.input == self.output {
            data
        } else {
            self.scratch.clear();
            for frame in data.chunks_exact(self.input) {
                self.scratch.extend_from_slice(&frame[..self.output]);
            }
            &self.scratch
        };
        self.writer
            .write(write)
            .map_err(|e| format!("Recording stopped: {e}"))
    }

    pub fn seconds(&self) -> f64 {
        self.writer.frames() as f64 / f64::from(self.rate.max(1))
    }

    /// Finish the file; returns its length in seconds.
    pub fn finish(self) -> Result<f64, String> {
        let seconds = self.seconds();
        self.writer
            .finish()
            .map_err(|e| format!("Cannot finish the recording: {e}"))?;
        Ok(seconds)
    }
}

/// Write buffers from `buffers` until `stop` is set and they are drained;
/// returns the recording's length in seconds.
pub(crate) fn pump(
    buffers: &Receiver<Vec<f32>>,
    mut capture: Capture,
    stop: &AtomicBool,
) -> Result<f64, String> {
    loop {
        match buffers.recv_timeout(Duration::from_millis(40)) {
            Ok(data) => capture.push(&data)?,
            Err(RecvTimeoutError::Timeout) if !stop.load(Ordering::Relaxed) => {}
            Err(_) => break,
        }
        if stop.load(Ordering::Relaxed) {
            while let Ok(data) = buffers.try_recv() {
                capture.push(&data)?;
            }
            break;
        }
    }
    capture.finish()
}

/// A finished recording: a WAV file to import. The file is removed when
/// this is dropped, so a take that is never used leaves nothing behind.
#[derive(Debug)]
pub(crate) struct Recorded {
    pub path: PathBuf,
    pub seconds: f64,
}

impl Drop for Recorded {
    fn drop(&mut self) {
        std::fs::remove_file(&self.path).ok();
    }
}

/// A recording in progress, until [`Recording::stop`].
pub(crate) struct Recording {
    pub device: String,
    pub meter: Arc<Meter>,
    started: Instant,
    stop: Arc<AtomicBool>,
    error: Arc<Mutex<Option<String>>>,
    done: Option<std::thread::JoinHandle<Result<f64, String>>>,
    path: PathBuf,
}

/// Why the system refused, in words for the status bar.
fn device_error(error: &cpal::Error) -> String {
    use cpal::ErrorKind as K;
    match error.kind() {
        K::PermissionDenied => "Emulsion may not use the microphone. Allow microphone access for Emulsion in your system's privacy settings, then try again.".into(),
        K::DeviceNotAvailable => "The audio input device is not available. Connect it, or choose another in Settings → Storyboard → Audio input device.".into(),
        K::DeviceBusy => "The audio input device is busy in another app.".into(),
        _ => format!("The microphone could not be opened: {error}"),
    }
}

fn device_name(device: &cpal::Device) -> Option<String> {
    use cpal::traits::DeviceTrait;
    device.description().ok().map(|d| d.name().to_string())
}

/// The input devices' names, the system default first.
pub(crate) fn input_devices() -> Result<Vec<String>, String> {
    use cpal::traits::HostTrait;
    let host = cpal::default_host();
    let default = host.default_input_device().and_then(|d| device_name(&d));
    let mut names: Vec<String> = host
        .input_devices()
        .map_err(|e| device_error(&e))?
        .filter_map(|d| device_name(&d))
        .collect();
    names.dedup();
    if let Some(default) = default {
        names.retain(|n| *n != default);
        names.insert(0, default);
    }
    Ok(names)
}

fn open_device(name: Option<&str>) -> Result<cpal::Device, String> {
    use cpal::traits::HostTrait;
    let host = cpal::default_host();
    match name {
        None => host.default_input_device().ok_or_else(|| {
            "No microphone found. Connect one, or check that sound input is enabled.".to_string()
        }),
        Some(name) => host
            .input_devices()
            .map_err(|e| device_error(&e))?
            .find(|d| device_name(d).as_deref() == Some(name))
            .ok_or_else(|| {
                format!("The audio input “{name}” is not connected. Connect it, or choose another in Settings → Storyboard → Audio input device.")
            }),
    }
}

impl Recording {
    /// Start recording from the input device named `device` (the system
    /// default when `None`) into a new file in the media cache.
    pub fn start(device: Option<&str>) -> Result<Self, String> {
        let path = emulsion_io::audio::store::cache_path("wav").map_err(|e| e.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let meter = Arc::new(Meter::default());
        let error = Arc::new(Mutex::new(None));
        let (opened, result) = std::sync::mpsc::channel();
        let device = device.map(str::to_string);
        let (file, flag, level, failed) =
            (path.clone(), stop.clone(), meter.clone(), error.clone());
        let done = std::thread::Builder::new()
            .name("emulsion-record".into())
            .spawn(move || {
                let (send, buffers) = std::sync::mpsc::channel();
                let started = open_device(device.as_deref()).and_then(|d| {
                    let name = device_name(&d).unwrap_or_else(|| "Microphone".into());
                    start_stream(&d, send, level, failed)
                        .map(|(stream, rate, channels)| (stream, rate, channels, name))
                });
                let (stream, rate, channels, name) = match started {
                    Ok(started) => started,
                    Err(error) => {
                        opened.send(Err(error.clone())).ok();
                        return Err(error);
                    }
                };
                let capture = match Capture::new(&file, rate, channels) {
                    Ok(capture) => capture,
                    Err(error) => {
                        opened.send(Err(error.clone())).ok();
                        return Err(error);
                    }
                };
                opened.send(Ok(name)).ok();
                let written = pump(&buffers, capture, &flag);
                drop(stream);
                written
            })
            .map_err(|e| e.to_string())?;
        let name = result
            .recv()
            .map_err(|_| "The audio input stopped while opening.".to_string())?;
        let name = match name {
            Ok(name) => name,
            Err(error) => {
                done.join().ok();
                std::fs::remove_file(&path).ok();
                return Err(error);
            }
        };
        Ok(Self {
            device: name,
            meter,
            started: Instant::now(),
            stop,
            error,
            done: Some(done),
            path,
        })
    }

    /// Seconds recorded so far.
    pub fn elapsed(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }

    /// Why the device failed while recording, if it did.
    pub fn error(&self) -> Option<String> {
        self.error.lock().clone()
    }

    /// Stop and finish the file.
    pub fn stop(mut self) -> Result<Recorded, String> {
        self.stop.store(true, Ordering::Relaxed);
        let seconds = self
            .done
            .take()
            .and_then(|done| done.join().ok())
            .unwrap_or_else(|| Err("The recording stopped unexpectedly.".into()));
        match seconds {
            Ok(seconds) if seconds > 0. => Ok(Recorded {
                path: self.path.clone(),
                seconds,
            }),
            Ok(_) => {
                std::fs::remove_file(&self.path).ok();
                Err(self.error().unwrap_or_else(|| {
                    "Nothing was recorded: the microphone sent no sound.".into()
                }))
            }
            Err(error) => {
                std::fs::remove_file(&self.path).ok();
                Err(error)
            }
        }
    }
}

impl Drop for Recording {
    /// A recording dropped without `stop` is discarded.
    fn drop(&mut self) {
        if let Some(done) = self.done.take() {
            self.stop.store(true, Ordering::Relaxed);
            done.join().ok();
            std::fs::remove_file(&self.path).ok();
        }
    }
}

type Started = (cpal::Stream, u32, u16);

fn start_stream(
    device: &cpal::Device,
    send: Sender<Vec<f32>>,
    meter: Arc<Meter>,
    error: Arc<Mutex<Option<String>>>,
) -> Result<Started, String> {
    use cpal::SampleFormat as F;
    use cpal::traits::{DeviceTrait, StreamTrait};
    let supported = device
        .default_input_config()
        .map_err(|e| device_error(&e))?;
    let config = supported.config();
    let stream = match supported.sample_format() {
        F::F32 => build::<f32>(device, &config, send, meter, error),
        F::I16 => build::<i16>(device, &config, send, meter, error),
        F::U16 => build::<u16>(device, &config, send, meter, error),
        F::I32 => build::<i32>(device, &config, send, meter, error),
        F::U8 => build::<u8>(device, &config, send, meter, error),
        F::F64 => build::<f64>(device, &config, send, meter, error),
        other => return Err(format!("Unsupported microphone sample format {other}.")),
    }?;
    stream.play().map_err(|e| device_error(&e))?;
    Ok((stream, config.sample_rate, config.channels))
}

fn build<T: cpal::SizedSample>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    send: Sender<Vec<f32>>,
    meter: Arc<Meter>,
    error: Arc<Mutex<Option<String>>>,
) -> Result<cpal::Stream, String>
where
    f32: cpal::FromSample<T>,
{
    use cpal::traits::DeviceTrait;
    device
        .build_input_stream(
            *config,
            move |data: &[T], _: &cpal::InputCallbackInfo| {
                let samples: Vec<f32> = data
                    .iter()
                    .map(|s| <f32 as cpal::FromSample<T>>::from_sample_(*s))
                    .collect();
                meter.feed(peak(&samples));
                send.send(samples).ok();
            },
            move |e| {
                tracing::warn!("audio input: {e}");
                *error.lock() = Some(device_error(&e));
            },
            None,
        )
        .map_err(|e| device_error(&e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_i16(path: &Path) -> (u16, u32, Vec<i16>) {
        let bytes = std::fs::read(path).unwrap();
        let channels = u16::from_le_bytes([bytes[22], bytes[23]]);
        let rate = u32::from_le_bytes(bytes[24..28].try_into().unwrap());
        let samples = bytes[44..]
            .chunks(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        (channels, rate, samples)
    }

    #[test]
    fn captures_keep_mono_and_the_first_two_channels() {
        let dir = tempfile::tempdir().unwrap();
        let mono = dir.path().join("mono.wav");
        let mut c = Capture::new(&mono, 1000, 1).unwrap();
        c.push(&[0.5, -0.5]).unwrap();
        assert!((c.seconds() - 0.002).abs() < 1e-9);
        c.finish().unwrap();
        assert_eq!(read_i16(&mono), (1, 1000, vec![16384, -16384]));

        let four = dir.path().join("four.wav");
        let mut c = Capture::new(&four, 48_000, 4).unwrap();
        c.push(&[0.1, 0.2, 0.9, 0.9, 0.3, 0.4, 0.9, 0.9]).unwrap();
        c.finish().unwrap();
        let (channels, _, samples) = read_i16(&four);
        assert_eq!(channels, 2);
        assert_eq!(samples.len(), 4);
        assert_eq!(samples[1], (0.2f32 * 32767.).round() as i16);
    }

    #[test]
    fn the_pump_writes_every_buffer_then_finishes_on_stop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("take.wav");
        let capture = Capture::new(&path, 100, 2).unwrap();
        let (send, buffers) = std::sync::mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let writer = std::thread::spawn(move || pump(&buffers, capture, &flag));
        for _ in 0..10 {
            send.send(vec![0.25; 20]).unwrap();
        }
        stop.store(true, Ordering::Relaxed);
        send.send(vec![0.25; 20]).unwrap();
        let seconds = writer.join().unwrap().unwrap();
        // Buffers sent before the writer noticed the stop are all kept.
        assert!((1.0..=1.1).contains(&seconds), "{seconds}");
        let (_, _, samples) = read_i16(&path);
        assert!(samples.iter().all(|s| *s == 8192));
    }

    #[test]
    fn the_meter_reads_the_peak_since_last_read() {
        let meter = Meter::default();
        meter.feed(peak(&[0.1, -0.6, 0.3]));
        meter.feed(0.2);
        assert_eq!(meter.take(), 0.6);
        assert_eq!(meter.take(), 0.);
        meter.feed(3.);
        assert_eq!(meter.take(), 1.);
    }
}
