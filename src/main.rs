mod effects;
mod live;
mod params;

use effects::{parse_spec, Chain, Spec, EFFECT_NAMES};
use std::process::ExitCode;

const DEFAULT_BUFFER: u32 = 64;

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
                mode high|fast (high)  window ms 4..40 (10; fast mode only)
  --vocoder     mix* 0..1 (1)       bands 4..32 (16)  carrier Hz (110)  release ms (15)
  --autotune    amount* 0..1 (1)    key C..B (C)  scale chromatic|major|minor|
                pentatonic|minor-pentatonic|blues (chromatic)  speed ms (40; 0 = hard tune)
                mode high|fast (high)  window ms 4..40 (10; fast mode only)
  --tremolo     depth* 0..1 (0.5)   rate Hz (5)
  --flanger     depth* 0..1 (0.7)   rate Hz (0.3)  feedback -0.95..0.95 (0.5)  mix 0..1 (0.5)
  --phaser      depth* 0..1 (0.7)   rate Hz (0.5)  feedback -0.95..0.95 (0.5)  mix 0..1 (0.5)
  --wah         sensitivity* 0..1 (0.6)  rate Hz (0 = follows your picking)  q 1..15 (5)
                min Hz (350)  max Hz (2200)  mix 0..1 (1)             (auto-wah)
  --reverb      mix* 0..1 (0.3)     time s (2)  size m (10)  damping 0..1 (0.5)

Pitch effects: mode=high is clean but adds 50+ ms and works on chords; mode=fast is
pitch-synchronous: about 10 ms, accurate on a single note, and degrades on chords.

OPTIONS
  -i, --input FILE          WAV file to process (omit for live mode)
  -o, --output FILE         output WAV (default: <input>.fx.wav)
      --tail SECONDS        extra time rendered after a file ends, for echo tails (1.0)
      --input-device TEXT   live: pick input device by (part of) its id
      --output-device TEXT  live: pick output device by (part of) its id
      --buffer FRAMES       live: PipeWire buffer in frames, smaller = lower latency (64)
      --channel N           live: input channel to use (0)
      -v, --verbose         live: print input/output levels every second
      --list-devices        list audio devices
      --input-gain DB       gain before the chain, -60..40 (0)
      --output-gain DB      gain after the chain (0)
      --preset FILE         load effects from a preset file (see below)
      --save-preset FILE    write the current chain to a preset file
  -h, --help

PRESETS
  One `name value` per line, `#` for comments; names are the effect names plus
  input-gain, output-gain and buffer. Example:
      compressor ratio=3
      overdrive drive=0.6,tone=0.5
      echo time=380,mix=0.3
      output-gain -3
";

struct Cli {
    specs: Vec<Spec>,
    input: Option<String>,
    output: Option<String>,
    tail: f32,
    save_preset: Option<String>,
    live: live::LiveOpts,
    list_devices: bool,
}

fn db_value(v: &str, flag: &str) -> Result<f32, String> {
    let n: f32 = v.trim_end_matches("dB").trim_end_matches("db").parse().map_err(|_| format!("{flag}: '{v}' is not a number (dB)"))?;
    if !(-60.0..=40.0).contains(&n) {
        return Err(format!("{flag}: {n} dB is out of range (-60..40)"));
    }
    Ok(n)
}

/// Preset files: one `name [value]` per line (`#` starts a comment). Names are the
/// effect names plus `input-gain`, `output-gain` and `buffer`. Effects keep file order
/// and are appended to the chain, so presets and flags can be combined.
fn load_preset(path: &str, cli: &mut Cli) -> Result<(), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    for (n, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let (name, value) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
        let (name, value) = (name.trim_start_matches("--"), value.trim());
        let at = |e: String| format!("{path}:{}: {e}", n + 1);
        match name {
            "input-gain" => cli.live.input_db = db_value(value, name).map_err(at)?,
            "output-gain" => cli.live.output_db = db_value(value, name).map_err(at)?,
            "buffer" => cli.live.buffer = Some(value.parse().map_err(|_| at("buffer: not a number".into()))?),
            _ => cli.specs.push(parse_spec(name, value).map_err(at)?),
        }
    }
    Ok(())
}

fn save_preset(path: &str, cli: &Cli) -> Result<(), String> {
    let mut out = String::from("# fxchain preset — effects run top to bottom\n");
    if cli.live.input_db != 0.0 {
        out += &format!("input-gain {}\n", cli.live.input_db);
    }
    for s in &cli.specs {
        out += &format!("{} {}\n", s.name, s.raw).replace(" \n", "\n");
    }
    if cli.live.output_db != 0.0 {
        out += &format!("output-gain {}\n", cli.live.output_db);
    }
    std::fs::write(path, out).map_err(|e| format!("{path}: {e}"))?;
    eprintln!("saved preset {path}");
    Ok(())
}

fn parse_args() -> Result<Option<Cli>, String> {
    let mut cli = Cli {
        specs: Vec::new(),
        input: None,
        output: None,
        tail: 1.0,
        save_preset: None,
        live: live::LiveOpts {
            input_device: None,
            output_device: None,
            buffer: Some(DEFAULT_BUFFER),
            channel: 0,
            verbose: false,
            input_db: 0.0,
            output_db: 0.0,
        },
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
            "--input-gain" => cli.live.input_db = db_value(&value(false)?, &flag)?,
            "--output-gain" => cli.live.output_db = db_value(&value(false)?, &flag)?,
            "--preset" => load_preset(&value(false)?, &mut cli)?,
            "--save-preset" => cli.save_preset = Some(value(false)?),
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
        let mut chain = Chain::new(&cli.specs, sr, cli.live.input_db, cli.live.output_db)?;
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
    if let Some(path) = &cli.save_preset {
        save_preset(path, &cli)?;
    }
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
