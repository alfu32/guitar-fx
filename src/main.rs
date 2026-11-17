mod effects;
mod live;
mod params;

use effects::{parse_spec, Chain, Spec, EFFECT_NAMES};
use std::process::ExitCode;

const HELP: &str = "\
fxchain — an ordered, repeatable multi-effect chain

USAGE
  fxchain [OPTIONS] [EFFECTS...]
  fxchain --input in.wav --output out.wav [EFFECTS...]    process a file
  fxchain [EFFECTS...]                                     live: input -> effects -> output

Effects run in the order given and may be repeated, e.g.
  fxchain --compressor ratio=4 --overdrive drive=0.7 --echo time=400 --overdrive 0.2

Each effect takes name=value pairs separated by commas. A bare number sets the
first parameter listed (marked *).

  --overdrive   drive* 0..1 (0.5)   tone 0..1 (0.6)   level 0..2 (1)
  --compressor  ratio* 1..20 (4)    threshold dB (-18)  attack ms (10)
                release ms (120)    makeup dB|auto (auto)
  --echo        mix* 0..1 (0.4)     time ms (350)  feedback 0..0.95 (0.45)  tone 0..1 (0.5)
  --delay       time* ms (300)      feedback 0..0.95 (0.3)  mix 0..1 (0.5)
  --chorus      depth* 0..1 (0.5)   rate Hz (0.6)  mix 0..1 (0.5)
  --harmonics   mix* 0..1 (0.3)     drive 0..1 (0.5)  freq Hz (2000)     (exciter)
  --tone-shift  semitones* -24..24 (0)  cents -100..100 (0)  mix 0..1 (1)  (pitch shifter)
  --vocoder     mix* 0..1 (1)       bands 4..32 (16)  carrier Hz (110)  release ms (15)
  --autotune    amount* 0..1 (1)    key C..B (C)  scale chromatic|major|minor|
                pentatonic|minor-pentatonic|blues (chromatic)  speed ms (40; 0 = hard tune)

OPTIONS
  -i, --input FILE          WAV file to process (omit for live mode)
  -o, --output FILE         output WAV (default: <input>.fx.wav)
      --tail SECONDS        extra time rendered after a file ends, for echo tails (1.0)
      --input-device TEXT   live: pick input device by (part of) its id
      --output-device TEXT  live: pick output device by (part of) its id
      --buffer FRAMES       live: device buffer size, smaller = lower latency
      --channel N           live: input channel to use (0)
      -v, --verbose         live: print input/output levels every second
      --list-devices        list audio devices
  -h, --help
";

struct Cli {
    specs: Vec<Spec>,
    input: Option<String>,
    output: Option<String>,
    tail: f32,
    live: live::LiveOpts,
    list_devices: bool,
}

fn parse_args() -> Result<Option<Cli>, String> {
    let mut cli = Cli {
        specs: Vec::new(),
        input: None,
        output: None,
        tail: 1.0,
        live: live::LiveOpts { input_device: None, output_device: None, buffer: None, channel: 0, verbose: false },
        list_devices: false,
    };
    let mut args = std::env::args().skip(1).peekable();
    while let Some(arg) = args.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
            _ => (arg.clone(), None),
        };
        // Value is either inline (`--flag=v`) or the next argument. Effect flags may be
        // given bare to use all defaults.
        let is_effect = flag.strip_prefix("--").is_some_and(|n| EFFECT_NAMES.contains(&n));
        let mut value = |optional: bool| -> Result<String, String> {
            if let Some(v) = inline.clone() {
                return Ok(v);
            }
            match args.peek() {
                Some(next) if !(optional && next.starts_with("--")) => Ok(args.next().unwrap()),
                _ if optional => Ok(String::new()),
                _ => Err(format!("{flag} needs a value")),
            }
        };
        match flag.as_str() {
            "-h" | "--help" => return Ok(None),
            "-i" | "--input" => cli.input = Some(value(false)?),
            "-o" | "--output" => cli.output = Some(value(false)?),
            "--tail" => cli.tail = value(false)?.parse().map_err(|_| "--tail: not a number")?,
            "--input-device" => cli.live.input_device = Some(value(false)?),
            "--output-device" => cli.live.output_device = Some(value(false)?),
            "--buffer" => {
                cli.live.buffer = Some(value(false)?.parse().map_err(|_| "--buffer: not a number")?)
            }
            "--channel" => {
                cli.live.channel = value(false)?.parse().map_err(|_| "--channel: not a number")?
            }
            "--list-devices" => cli.list_devices = true,
            "-v" | "--verbose" => cli.live.verbose = true,
            _ if is_effect => {
                let raw = value(true)?;
                cli.specs.push(parse_spec(&flag[2..], &raw)?);
            }
            _ => return Err(format!("unknown option '{arg}' (see --help)")),
        }
    }
    Ok(Some(cli))
}

fn process_file(cli: &Cli, input: &str) -> Result<(), String> {
    let mut reader = hound::WavReader::open(input).map_err(|e| format!("{input}: {e}"))?;
    let spec = reader.spec();
    let channels = spec.channels as usize;
    let sr = spec.sample_rate as f32;

    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => {
            reader.samples::<f32>().collect::<Result<_, _>>().map_err(|e| e.to_string())?
        }
        hound::SampleFormat::Int => {
            let scale = (1u64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / scale))
                .collect::<Result<_, _>>()
                .map_err(|e| e.to_string())?
        }
    };

    // De-interleave, add the tail, and run one chain per channel.
    let frames = samples.len() / channels + (cli.tail.max(0.0) * sr) as usize;
    let mut planes: Vec<Vec<f32>> = (0..channels)
        .map(|c| {
            let mut v: Vec<f32> = samples.iter().skip(c).step_by(channels).copied().collect();
            v.resize(frames, 0.0);
            v
        })
        .collect();
    for plane in &mut planes {
        let mut chain = Chain::new(&cli.specs, sr)?;
        for block in plane.chunks_mut(256) {
            chain.process(block);
        }
    }

    let out_path = cli.output.clone().unwrap_or_else(|| {
        let stem = input.strip_suffix(".wav").unwrap_or(input);
        format!("{stem}.fx.wav")
    });
    let out_spec = hound::WavSpec {
        channels: spec.channels,
        sample_rate: spec.sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(&out_path, out_spec).map_err(|e| e.to_string())?;
    for i in 0..frames {
        for plane in &planes {
            writer.write_sample(plane[i]).map_err(|e| e.to_string())?;
        }
    }
    writer.finalize().map_err(|e| e.to_string())?;
    let peak = planes.iter().flatten().fold(0.0f32, |m, x| m.max(x.abs()));
    eprintln!("wrote {out_path} ({frames} frames, peak {peak:.2}{})", if peak > 1.0 { " — clipping!" } else { "" });
    Ok(())
}

fn run() -> Result<(), String> {
    let Some(cli) = parse_args()? else {
        print!("{HELP}");
        return Ok(());
    };
    if cli.list_devices {
        return live::list_devices();
    }
    match &cli.input {
        Some(input) => process_file(&cli, input),
        None => {
            // PipeWire ignores the buffer size requested through ALSA and uses its own
            // quantum (1024 frames by default); PIPEWIRE_LATENCY is what it honors.
            if let Some(frames) = cli.live.buffer {
                if std::env::var_os("PIPEWIRE_LATENCY").is_none() {
                    // Safe: nothing else is running yet, this is still single-threaded.
                    unsafe { std::env::set_var("PIPEWIRE_LATENCY", format!("{frames}/48000")) };
                }
            }
            live::run(&cli.specs, &cli.live)
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
