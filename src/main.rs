use fxcore::chain::Chain;
use fxcore::live::{self, LiveOpts};
use fxcore::params::{effect_def, Kind, Spec, EFFECTS};
use fxcore::preset::{parse_db, Preset};
use std::process::ExitCode;

fn help() -> String {
    let mut s = String::from(
        "\
fxchain — an ordered, repeatable multi-effect chain

USAGE
  fxchain [OPTIONS] [EFFECTS...]
  fxchain --input in.wav --output out.wav [EFFECTS...]    process a file
  fxchain [EFFECTS...]                                     live: input -> effects -> output
  fxchain-ui                                               the graphical version

Effects run in the order given and may be repeated, e.g.
  fxchain --compressor ratio=4 --overdrive drive=0.7 --echo time=400 --overdrive 0.2

Each effect takes name=value pairs separated by commas. A bare number sets the
first parameter listed (marked *). Defaults are in parentheses.

",
    );
    for def in EFFECTS {
        let params: Vec<String> = def
            .params
            .iter()
            .map(|p| {
                let star = if p.key == def.primary { "*" } else { "" };
                match &p.kind {
                    Kind::Num { min, max, default, unit } => {
                        format!("{}{star} {min}..{max}{unit} ({default})", p.key)
                    }
                    Kind::AutoNum { min, max, unit } => format!("{} {min}..{max}{unit} (auto)", p.key),
                    Kind::Choice { options, default } => {
                        format!("{} {} ({})", p.key, options.join("|"), options[*default])
                    }
                }
            })
            .collect();
        let mut line = format!("  --{:<11}", def.name);
        let indent = " ".repeat(line.len());
        let mut width = line.len();
        for (i, p) in params.iter().enumerate() {
            if width + p.len() > 96 && width > indent.len() {
                line = line.trim_end().to_string();
                line += &format!("\n{indent}");
                width = indent.len();
            }
            line += p;
            width += p.len();
            if i + 1 < params.len() {
                line += "  ";
                width += 2;
            }
        }
        s += &line;
        s += "\n";
    }
    s += "
Pitch effects: mode=high is clean but adds 50+ ms and works on chords; mode=fast is
pitch-synchronous: about 10 ms, accurate on one note, and degrades on chords.

OPTIONS
  -i, --input FILE          WAV file to process (omit for live mode)
  -o, --output FILE         output WAV (default: <input>.fx.wav)
      --tail SECONDS        extra time rendered after a file ends, for echo tails (1.0)
      --input-gain DB       gain before the chain, -60..40 (0)
      --output-gain DB      gain after the chain (0)
      --preset FILE         load effects from a preset file (see below)
      --save-preset FILE    write the current chain to a preset file
      --input-device TEXT   live: pick input device by (part of) its id
      --output-device TEXT  live: pick output device by (part of) its id
      --buffer FRAMES       live: buffer in frames, smaller = lower latency (64)
      --channel N           live: input channel to use (0)
  -v, --verbose             live: print input/output levels every second
      --list-devices        list audio devices
  -h, --help

PRESETS
  One `name value` per line, `#` for comments; names are the effect names plus
  input-gain, output-gain and buffer. Example:
      compressor ratio=3
      overdrive drive=0.6,tone=0.5
      echo time=380,mix=0.3
      output-gain -3
";
    s
}

struct Cli {
    specs: Vec<Spec>,
    input: Option<String>,
    output: Option<String>,
    tail: f32,
    save_preset: Option<String>,
    live: LiveOpts,
    verbose: bool,
    list_devices: bool,
}

fn parse_args() -> Result<Option<Cli>, String> {
    let mut cli = Cli {
        specs: Vec::new(),
        input: None,
        output: None,
        tail: 1.0,
        save_preset: None,
        live: LiveOpts::default(),
        verbose: false,
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
        let effect = flag.strip_prefix("--").filter(|n| effect_def(n).is_some());
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
            "--buffer" => cli.live.buffer = Some(value(false)?.parse().map_err(|_| "--buffer: not a number")?),
            "--channel" => cli.live.channel = value(false)?.parse().map_err(|_| "--channel: not a number")?,
            "--input-gain" => cli.live.input_db = parse_db(&value(false)?, &flag)?,
            "--output-gain" => cli.live.output_db = parse_db(&value(false)?, &flag)?,
            "--preset" => {
                let preset = Preset::load(&value(false)?)?;
                cli.specs.extend(preset.specs);
                cli.live.input_db = preset.input_db;
                cli.live.output_db = preset.output_db;
                if preset.buffer.is_some() {
                    cli.live.buffer = preset.buffer;
                }
            }
            "--save-preset" => cli.save_preset = Some(value(false)?),
            "--list-devices" => cli.list_devices = true,
            "-v" | "--verbose" => cli.verbose = true,
            _ if effect.is_some() => {
                let raw = value(true)?;
                cli.specs.push(Spec::parse(effect.unwrap(), &raw)?);
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
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>().map_err(|e| e.to_string())?,
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
        let mut chain = Chain::new(&cli.specs, sr, cli.live.input_db, cli.live.output_db);
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

fn run_live(cli: &Cli) -> Result<(), String> {
    let running = live::start(&cli.live, &cli.specs)?;
    eprintln!("input : {}\noutput: {}", running.input_name, running.output_name);
    eprintln!("running at {} Hz — press Ctrl-C to stop", running.control.sample_rate);
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
        if cli.verbose {
            let m = &running.control.meters;
            eprintln!("in {:.4}  out {:.4}  dropouts {}", m.in_peak(), m.out_peak(), m.xruns());
        }
    }
}

fn run() -> Result<(), String> {
    let Some(cli) = parse_args()? else {
        print!("{}", help());
        return Ok(());
    };
    if let Some(path) = &cli.save_preset {
        let preset = Preset {
            specs: cli.specs.clone(),
            input_db: cli.live.input_db,
            output_db: cli.live.output_db,
            buffer: None,
        };
        preset.save(path)?;
        eprintln!("saved preset {path}");
    }
    if cli.list_devices {
        let (ins, outs) = live::device_names()?;
        println!("inputs:");
        ins.iter().for_each(|d| println!("  {d}"));
        println!("outputs:");
        outs.iter().for_each(|d| println!("  {d}"));
        return Ok(());
    }
    match &cli.input {
        Some(input) => process_file(&cli, input),
        None => run_live(&cli),
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
