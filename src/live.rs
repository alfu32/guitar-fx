//! Live mode: default (or chosen) input -> effect chain -> output, via cpal.

use crate::effects::{Chain, Spec};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{BufferSize, Device, SampleFormat, SizedSample, StreamConfig};
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::HeapRb;
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
use std::sync::Arc;

pub struct LiveOpts {
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    pub buffer: Option<u32>,
    pub channel: usize,
    pub verbose: bool,
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

fn pick(devices: impl Iterator<Item = Device>, wanted: &str) -> Result<Device, String> {
    let wanted = wanted.to_lowercase();
    for d in devices {
        if d.id().map(|i| i.to_string().to_lowercase().contains(&wanted)).unwrap_or(false) {
            return Ok(d);
        }
    }
    Err(format!("no audio device matches '{wanted}' (see --list-devices)"))
}

pub fn list_devices() -> Result<(), String> {
    let host = cpal::default_host();
    println!("inputs:");
    for d in host.input_devices().map_err(err)? {
        println!("  {}", d.id().map(|i| i.to_string()).unwrap_or_default());
    }
    println!("outputs:");
    for d in host.output_devices().map_err(err)? {
        println!("  {}", d.id().map(|i| i.to_string()).unwrap_or_default());
    }
    Ok(())
}

pub fn run(specs: &[Spec], opts: &LiveOpts) -> Result<(), String> {
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
    eprintln!(
        "input : {}\noutput: {}",
        input.id().map(|i| i.to_string()).unwrap_or_default(),
        output.id().map(|i| i.to_string()).unwrap_or_default()
    );

    let buffer = opts.buffer.map(BufferSize::Fixed).unwrap_or(BufferSize::Default);
    let mut in_conf: StreamConfig = in_cfg.clone().into();
    in_conf.buffer_size = buffer;
    let mut out_conf: StreamConfig = out_cfg.clone().into();
    out_conf.sample_rate = in_conf.sample_rate; // run both ends at the input rate
    out_conf.buffer_size = buffer;
    let sr = in_conf.sample_rate as f32;

    macro_rules! go {
        ($t:ty) => {
            run_typed::<$t>(&input, &output, in_conf, out_conf, specs, sr, opts.channel, opts.verbose)
        };
    }
    match in_cfg.sample_format() {
        SampleFormat::F32 => go!(f32),
        SampleFormat::I16 => go!(i16),
        SampleFormat::I32 => go!(i32),
        f => Err(format!("unsupported input sample format {f}")),
    }?;
    Ok(())
}

fn run_typed<T>(
    input: &Device,
    output: &Device,
    in_conf: StreamConfig,
    out_conf: StreamConfig,
    specs: &[Spec],
    sr: f32,
    channel: usize,
    verbose: bool,
) -> Result<(), String>
where
    T: SizedSample + Send + 'static,
    f32: cpal::FromSample<T>,
    T: cpal::FromSample<f32>,
{
    use cpal::Sample;
    let mut chain = Chain::new(specs, sr)?;
    let in_ch = in_conf.channels as usize;
    let out_ch = out_conf.channels as usize;
    let channel = channel.min(in_ch - 1);

    let ring = HeapRb::<f32>::new(sr as usize / 2);
    let (mut producer, mut consumer) = ring.split();
    let mut scratch: Vec<f32> = Vec::new();
    // Meters (f32 bits): input rms, chain-output rms, and underrun count.
    let m_in = Arc::new(AtomicU32::new(0));
    let m_out = Arc::new(AtomicU32::new(0));
    let m_under = Arc::new(AtomicU32::new(0));
    let (m_in2, m_out2, m_under2) = (m_in.clone(), m_out.clone(), m_under.clone());

    let input_fn = move |data: &[T], _: &cpal::InputCallbackInfo| {
        scratch.clear();
        scratch.extend(data.chunks(in_ch).map(|f| f[channel].to_sample::<f32>()));
        let rms = |v: &[f32]| (v.iter().map(|x| x * x).sum::<f32>() / v.len().max(1) as f32).sqrt();
        m_in2.store(rms(&scratch).to_bits(), Relaxed);
        chain.process(&mut scratch);
        m_out2.store(rms(&scratch).to_bits(), Relaxed);
        producer.push_slice(&scratch);
    };
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
        for frame in data.chunks_mut(out_ch) {
            let s = consumer.try_pop().unwrap_or_else(|| {
                m_under2.fetch_add(1, Relaxed);
                0.0
            });
            frame.fill(s.clamp(-1.0, 1.0).to_sample::<T>());
        }
    };
    let on_err = |e: cpal::Error| eprintln!("stream error: {e}");

    let in_stream = input.build_input_stream(in_conf, input_fn, on_err, None).map_err(err)?;
    let out_stream = output.build_output_stream(out_conf, output_fn, on_err, None).map_err(err)?;
    in_stream.play().map_err(err)?;
    out_stream.play().map_err(err)?;
    eprintln!("running at {sr} Hz — press Ctrl-C to stop");
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        if verbose {
            eprintln!(
                "in {:.4}  out {:.4}  underruns {}",
                f32::from_bits(m_in.load(Relaxed)),
                f32::from_bits(m_out.load(Relaxed)),
                m_under.load(Relaxed)
            );
        }
    }
}
