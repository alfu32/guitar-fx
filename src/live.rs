//! Live mode: input device -> effect chain -> output device, via cpal.

use crate::chain::{Chain, Control, Meters};
use crate::params::Spec;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, Device, SampleFormat, SizedSample, Stream, StreamConfig};
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::HeapRb;
use std::sync::Arc;

pub const DEFAULT_BUFFER: u32 = 64;

#[derive(Clone)]
pub struct LiveOpts {
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub buffer: Option<u32>,
    pub channel: usize,
    pub input_db: f32,
    pub output_db: f32,
}

impl Default for LiveOpts {
    fn default() -> Self {
        Self {
            input_device: None,
            output_device: None,
            buffer: Some(DEFAULT_BUFFER),
            channel: 0,
            input_db: 0.0,
            output_db: 0.0,
        }
    }
}

/// A running live session. Dropping it stops the audio.
pub struct Running {
    _input: Stream,
    _output: Stream,
    pub control: Control,
    pub input_name: String,
    pub output_name: String,
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn name_of(d: &Device) -> String {
    d.id().map(|i| i.to_string()).unwrap_or_default()
}

fn pick(devices: impl Iterator<Item = Device>, wanted: &str) -> Result<Device, String> {
    let wanted = wanted.to_lowercase();
    for d in devices {
        if name_of(&d).to_lowercase().contains(&wanted) {
            return Ok(d);
        }
    }
    Err(format!("no audio device matches '{wanted}' (see --list-devices)"))
}

/// (inputs, outputs)
pub fn device_names() -> Result<(Vec<String>, Vec<String>), String> {
    let host = cpal::default_host();
    let ins = host.input_devices().map_err(err)?.map(|d| name_of(&d)).collect();
    let outs = host.output_devices().map_err(err)?.map(|d| name_of(&d)).collect();
    Ok((ins, outs))
}

/// PipeWire ignores the buffer size requested through ALSA and uses its own quantum
/// (1024 frames by default); `PIPEWIRE_LATENCY` is what it honors. Linux only; other
/// platforms honor the requested buffer size directly.
#[cfg(target_os = "linux")]
fn request_latency(frames: u32) {
    use std::sync::OnceLock;
    static USER_SET: OnceLock<bool> = OnceLock::new();
    let user_set = *USER_SET.get_or_init(|| std::env::var_os("PIPEWIRE_LATENCY").is_some());
    if !user_set {
        // SAFETY: called from the thread that starts audio, before the streams exist;
        // only PipeWire's client library reads this variable.
        unsafe { std::env::set_var("PIPEWIRE_LATENCY", format!("{frames}/48000")) };
    }
}

#[cfg(not(target_os = "linux"))]
fn request_latency(_frames: u32) {}

pub fn start(opts: &LiveOpts, specs: &[Spec]) -> Result<Running, String> {
    if let Some(frames) = opts.buffer {
        request_latency(frames);
    }
    let host = cpal::default_host();
    let input = match &opts.input_device {
        Some(w) => pick(host.input_devices().map_err(err)?, w)?,
        None => host.default_input_device().ok_or("no default input device")?,
    };
    let output = match &opts.output_device {
        Some(w) => pick(host.output_devices().map_err(err)?, w)?,
        None => host.default_output_device().ok_or("no default output device")?,
    };
    let in_cfg = input.default_input_config().map_err(err)?;
    let out_cfg = output.default_output_config().map_err(err)?;

    let buffer = opts.buffer.map(BufferSize::Fixed).unwrap_or(BufferSize::Default);
    let mut in_conf: StreamConfig = in_cfg.clone().into();
    in_conf.buffer_size = buffer;
    let mut out_conf: StreamConfig = out_cfg.clone().into();
    out_conf.sample_rate = in_conf.sample_rate; // run both ends at the input rate
    out_conf.buffer_size = buffer;
    let sr = in_conf.sample_rate as f32;

    let (chain, control) = Chain::controlled(specs, sr, opts.input_db, opts.output_db);
    let channel = opts.channel;

    macro_rules! go {
        ($t:ty) => {
            run_typed::<$t>(&input, &output, in_conf, out_conf, chain, channel)
        };
    }
    let (in_stream, out_stream) = match in_cfg.sample_format() {
        SampleFormat::F32 => go!(f32),
        SampleFormat::I16 => go!(i16),
        SampleFormat::I32 => go!(i32),
        f => Err(format!("unsupported input sample format {f}")),
    }?;
    Ok(Running {
        _input: in_stream,
        _output: out_stream,
        control,
        input_name: name_of(&input),
        output_name: name_of(&output),
    })
}

fn run_typed<T>(
    input: &Device,
    output: &Device,
    in_conf: StreamConfig,
    out_conf: StreamConfig,
    mut chain: Chain,
    channel: usize,
) -> Result<(Stream, Stream), String>
where
    T: SizedSample + Send + 'static,
    f32: cpal::FromSample<T>,
    T: cpal::FromSample<f32>,
{
    use cpal::Sample;
    let sr = in_conf.sample_rate as usize;
    let in_ch = in_conf.channels as usize;
    let out_ch = out_conf.channels as usize;
    let channel = channel.min(in_ch - 1);
    let meters: Arc<Meters> = chain.meters();

    let ring = HeapRb::<f32>::new(sr / 2);
    let (mut producer, mut consumer) = ring.split();
    let mut scratch: Vec<f32> = Vec::new();

    let input_fn = move |data: &[T], _: &cpal::InputCallbackInfo| {
        scratch.clear();
        scratch.extend(data.chunks(in_ch).map(|f| f[channel].to_sample::<f32>()));
        chain.process(&mut scratch);
        producer.push_slice(&scratch);
    };

    let out_meters = meters.clone();
    let mut primed = false;
    let output_fn = move |data: &mut [T], _: &cpal::OutputCallbackInfo| {
        // Wait for a little input before starting so tiny jitters don't underrun.
        if !primed {
            if consumer.occupied_len() < data.len() / out_ch {
                data.fill(T::EQUILIBRIUM);
                return;
            }
            primed = true;
        }
        let mut starved = false;
        for frame in data.chunks_mut(out_ch) {
            let s = consumer.try_pop().unwrap_or_else(|| {
                starved = true;
                0.0
            });
            frame.fill(s.clamp(-1.0, 1.0).to_sample::<T>());
        }
        if starved {
            out_meters.add_xrun();
        }
    };

    let on_err = move |e: cpal::Error| {
        if matches!(e.kind(), cpal::ErrorKind::Xrun) {
            meters.add_xrun();
        } else {
            eprintln!("stream error: {e}");
        }
    };
    let on_err2 = on_err.clone();

    let in_stream = input.build_input_stream(in_conf, input_fn, on_err, None).map_err(err)?;
    let out_stream = output.build_output_stream(out_conf, output_fn, on_err2, None).map_err(err)?;
    in_stream.play().map_err(err)?;
    out_stream.play().map_err(err)?;
    Ok((in_stream, out_stream))
}
