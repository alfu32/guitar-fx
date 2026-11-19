//! Render a wav file through a .nam model: `nam_render MODEL.nam IN.wav OUT.wav [lite]`.
use guitar_fx_core::nam::Model;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 4 {
        eprintln!("usage: nam_render MODEL.nam IN.wav OUT.wav [lite]");
        std::process::exit(2);
    }
    let mut m = Model::load(std::path::Path::new(&a[1]), a.get(4).is_some_and(|s| s == "lite")).unwrap_or_else(|e| {
        eprintln!("{e}");
        std::process::exit(1);
    });
    let mut r = hound::WavReader::open(&a[2]).expect("input wav");
    let spec = r.spec();
    let max = (1i64 << (spec.bits_per_sample - 1)) as f32;
    let mut x: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => r.samples::<f32>().map(|s| s.unwrap()).collect(),
        hound::SampleFormat::Int => r.samples::<i32>().map(|s| s.unwrap() as f32 / max).collect(),
    };
    let t = std::time::Instant::now();
    m.process(&mut x);
    let secs = x.len() as f32 / spec.sample_rate as f32;
    eprintln!("{}: {:.2} s of audio in {:.3} s ({:.1}% of one core)", m.name, secs, t.elapsed().as_secs_f32(), 100.0 * t.elapsed().as_secs_f32() / secs);
    let mut w = hound::WavWriter::create(&a[3], hound::WavSpec { channels: 1, sample_rate: spec.sample_rate, bits_per_sample: 32, sample_format: hound::SampleFormat::Float }).unwrap();
    for s in x {
        w.write_sample(s).unwrap();
    }
    w.finalize().unwrap();
}
