//! Neural Amp Modeler (`.nam`) inference: a sample-by-sample WaveNet runner.
//!
//! Reads the current "slimmable container" files (a lite and a full WaveNet in one file)
//! as well as the older plain WaveNet files. It follows the reference implementation
//! (NeuralAmpModelerCore, MIT), including its weight order. FiLM and "blended" layers
//! are not supported and fail to load with a clear message.

use serde_json::Value;
use std::path::Path;

// ---------------------------------------------------------------- activations

#[derive(Clone, Copy, Debug)]
enum Act {
    Identity,
    Tanh,
    FastTanh,
    HardTanh,
    Relu,
    LeakyRelu(f32),
    Sigmoid,
    Silu,
    HardSwish,
    SoftSign,
}

impl Act {
    fn parse(v: &Value) -> Result<Act, String> {
        let (name, slope) = match v {
            Value::String(s) => (s.as_str(), None),
            Value::Object(o) => (
                o.get("type").and_then(Value::as_str).ok_or("activation without a type")?,
                o.get("negative_slope").and_then(Value::as_f64),
            ),
            Value::Null => ("Identity", None),
            _ => return Err("unreadable activation".into()),
        };
        Ok(match name {
            "Tanh" => Act::Tanh,
            "Fasttanh" => Act::FastTanh,
            "Hardtanh" => Act::HardTanh,
            "ReLU" => Act::Relu,
            "LeakyReLU" => Act::LeakyRelu(slope.unwrap_or(0.01) as f32),
            "Sigmoid" => Act::Sigmoid,
            "SiLU" => Act::Silu,
            "Hardswish" => Act::HardSwish,
            "Softsign" => Act::SoftSign,
            "Identity" => Act::Identity,
            other => return Err(format!("activation {other} is not supported")),
        })
    }

    #[inline(always)]
    fn apply(self, x: f32) -> f32 {
        match self {
            Act::Identity => x,
            Act::Tanh => x.tanh(),
            Act::FastTanh => {
                let ax = x.abs();
                let x2 = x * x;
                x * (2.455_507_5 + 2.455_507_5 * ax + (0.893_229_85 + 0.821_226_7 * ax) * x2)
                    / (2.445_066_4 + (2.445_066_4 + x2) * (x + 0.814_642_75 * x * ax).abs())
            }
            Act::HardTanh => x.clamp(-1.0, 1.0),
            Act::Relu => x.max(0.0),
            Act::LeakyRelu(s) => {
                if x > 0.0 {
                    x
                } else {
                    s * x
                }
            }
            Act::Sigmoid => 1.0 / (1.0 + (-x).exp()),
            Act::Silu => x / (1.0 + (-x).exp()),
            Act::HardSwish => x * (x + 3.0).clamp(0.0, 6.0) / 6.0,
            Act::SoftSign => x / (1.0 + x.abs()),
        }
    }
}

// ---------------------------------------------------------------- weights

struct Weights<'a> {
    w: &'a [f32],
    at: usize,
}

impl Weights<'_> {
    fn take(&mut self, n: usize) -> Result<&[f32], String> {
        let s = self.w.get(self.at..self.at + n).ok_or("the file has fewer weights than its architecture needs")?;
        self.at += n;
        Ok(s)
    }
}

// ---------------------------------------------------------------- building blocks
//
// The network runs on chunks of up to `CH` frames. Signals are channel-major (row `c`
// starts at `c * CH`), so each multiply-accumulate loop runs along time over contiguous
// memory, which the compiler turns into vector code.

const CH: usize = 128;

/// `out[t] = bias + sum_j w[j] * src[bases[j] + t]` for `t < n`, kept in registers 32 frames at a time.
#[inline(always)]
fn mac(out: &mut [f32], n: usize, bias: f32, w: &[f32], bases: &[usize], src: &[f32]) {
    let mut t0 = 0;
    while t0 + 32 <= n {
        let mut acc = [bias; 32];
        for (wj, &b) in w.iter().zip(bases) {
            let x = &src[b + t0..b + t0 + 32];
            for i in 0..32 {
                acc[i] += wj * x[i];
            }
        }
        out[t0..t0 + 32].copy_from_slice(&acc);
        t0 += 32;
    }
    while t0 + 16 <= n {
        let mut acc = [bias; 16];
        for (wj, &b) in w.iter().zip(bases) {
            let x = &src[b + t0..b + t0 + 16];
            for i in 0..16 {
                acc[i] += wj * x[i];
            }
        }
        out[t0..t0 + 16].copy_from_slice(&acc);
        t0 += 16;
    }
    while t0 + 4 <= n {
        let mut acc = [bias; 4];
        for (wj, &b) in w.iter().zip(bases) {
            let x = &src[b + t0..b + t0 + 4];
            for i in 0..4 {
                acc[i] += wj * x[i];
            }
        }
        out[t0..t0 + 4].copy_from_slice(&acc);
        t0 += 4;
    }
    while t0 < n {
        let mut a = bias;
        for (wj, &b) in w.iter().zip(bases) {
            a += wj * src[b + t0];
        }
        out[t0] = a;
        t0 += 1;
    }
}

/// A 1x1 convolution: `w[o * cin + c]`.
struct Dense {
    cin: usize,
    cout: usize,
    w: Vec<f32>,
    b: Vec<f32>,
    bases: Vec<usize>,
}

impl Dense {
    fn read(wt: &mut Weights, cin: usize, cout: usize, bias: bool, groups: usize) -> Result<Dense, String> {
        if groups == 0 || cin % groups != 0 || cout % groups != 0 {
            return Err("channel counts do not divide into the groups".into());
        }
        let (opg, ipg) = (cout / groups, cin / groups);
        let mut w = vec![0.0; cin * cout];
        for g in 0..groups {
            for i in 0..opg {
                for j in 0..ipg {
                    w[(g * opg + i) * cin + g * ipg + j] = wt.take(1)?[0];
                }
            }
        }
        let b = if bias { wt.take(cout)?.to_vec() } else { vec![0.0; cout] };
        Ok(Dense { cin, cout, w, b, bases: (0..cin).map(|c| c * CH).collect() })
    }

    /// `x` and `out` are channel-major with stride `CH`.
    #[inline(always)]
    fn apply(&self, x: &[f32], out: &mut [f32], n: usize) {
        for o in 0..self.cout {
            mac(&mut out[o * CH..o * CH + n], n, self.b[o], &self.w[o * self.cin..(o + 1) * self.cin], &self.bases, x);
        }
    }
}

/// The recent past of a signal: each channel keeps `reach` frames before the chunk being
/// processed, in a buffer that is moved back to the start now and then.
struct Hist {
    ch: usize,
    reach: usize,
    len: usize,
    pos: usize,
    buf: Vec<f32>,
}

impl Hist {
    fn new(ch: usize, reach: usize) -> Hist {
        let len = reach + 512 + CH;
        Hist { ch, reach, len, pos: reach, buf: vec![0.0; ch * len] }
    }

    /// Append a chunk (rows of stride `CH`).
    #[inline(always)]
    fn write(&mut self, x: &[f32], n: usize) {
        for c in 0..self.ch {
            let at = c * self.len + self.pos;
            self.buf[at..at + n].copy_from_slice(&x[c * CH..c * CH + n]);
        }
    }

    /// Start of channel `c`'s frames `back` frames before the current chunk, as an index into `buf`.
    #[inline(always)]
    fn at(&self, c: usize, back: usize) -> usize {
        c * self.len + self.pos - back
    }

    #[inline(always)]
    fn advance(&mut self, n: usize) {
        self.pos += n;
        if self.pos + CH > self.len {
            for c in 0..self.ch {
                let from = c * self.len + self.pos - self.reach;
                self.buf.copy_within(from..from + self.reach, c * self.len);
            }
            self.pos = self.reach;
        }
    }
}

/// A causal dilated convolution reading a `Hist`: `w[(o * k + tap) * cin + c]`.
struct Conv {
    k: usize,
    dil: usize,
    cin: usize,
    cout: usize,
    w: Vec<f32>,
    b: Vec<f32>,
    bases: Vec<usize>,
}

impl Conv {
    fn read(wt: &mut Weights, cin: usize, cout: usize, k: usize, dil: usize, bias: bool, groups: usize) -> Result<Conv, String> {
        if k == 0 || dil == 0 || groups == 0 || cin % groups != 0 || cout % groups != 0 {
            return Err("invalid convolution shape".into());
        }
        let (opg, ipg) = (cout / groups, cin / groups);
        let mut w = vec![0.0; k * cin * cout];
        for g in 0..groups {
            for i in 0..opg {
                for j in 0..ipg {
                    for tap in 0..k {
                        w[((g * opg + i) * k + tap) * cin + g * ipg + j] = wt.take(1)?[0];
                    }
                }
            }
        }
        let b = if bias { wt.take(cout)?.to_vec() } else { vec![0.0; cout] };
        Ok(Conv { k, dil, cin, cout, w, b, bases: vec![0; k * cin] })
    }

    fn reach(&self) -> usize {
        (self.k - 1) * self.dil
    }

    #[inline(always)]
    fn apply(&mut self, hist: &Hist, out: &mut [f32], n: usize) {
        for tap in 0..self.k {
            for c in 0..self.cin {
                self.bases[tap * self.cin + c] = hist.at(c, (self.k - 1 - tap) * self.dil);
            }
        }
        let kc = self.k * self.cin;
        for o in 0..self.cout {
            mac(&mut out[o * CH..o * CH + n], n, self.b[o], &self.w[o * kc..(o + 1) * kc], &self.bases, &hist.buf);
        }
    }
}

// ---------------------------------------------------------------- the network

struct Layer {
    conv: Conv,
    hist: Hist,
    mixin: Dense,
    layer1x1: Option<Dense>,
    head1x1: Option<Dense>,
    act: Act,
    /// `Some` for a gated layer: the activation of the gate half.
    gate: Option<Act>,
    bottleneck: usize,
    z: Vec<f32>,
    mix: Vec<f32>,
    skip: Vec<f32>,
    next: Vec<f32>,
}

struct Array {
    rechannel: Dense,
    layers: Vec<Layer>,
    head_hist: Hist,
    head: Conv,
    head_acc: Vec<f32>,
    head_size: usize,
    chan: Vec<f32>,
    out_layers: Vec<f32>,
    out_head: Vec<f32>,
}

impl Array {
    /// One chunk of `n` frames. `input` has `input_size` rows, `cond` the condition rows.
    #[inline(always)]
    fn step(&mut self, input: &[f32], cond: &[f32], head_in: Option<&[f32]>, n: usize) {
        self.rechannel.apply(input, &mut self.chan, n);
        let hin = self.head_acc.len() / CH;
        for r in 0..hin {
            let acc = &mut self.head_acc[r * CH..r * CH + n];
            match head_in {
                Some(h) => acc.copy_from_slice(&h[r * CH..r * CH + n]),
                None => acc.fill(0.0),
            }
        }
        for l in self.layers.iter_mut() {
            l.hist.write(&self.chan, n);
            l.conv.apply(&l.hist, &mut l.z, n);
            l.hist.advance(n);
            // add the condition (the dry input) through the mixin
            l.mixin.apply(cond, &mut l.mix, n);
            let bn = l.bottleneck;
            let zc = l.conv.cout;
            for o in 0..zc {
                for t in 0..n {
                    l.z[o * CH + t] += l.mix[o * CH + t];
                }
            }
            match l.gate {
                None => {
                    for o in 0..bn {
                        for z in &mut l.z[o * CH..o * CH + n] {
                            *z = l.act.apply(*z);
                        }
                    }
                }
                Some(gate) => {
                    for o in 0..bn {
                        for t in 0..n {
                            l.z[o * CH + t] = l.act.apply(l.z[o * CH + t]) * gate.apply(l.z[(bn + o) * CH + t]);
                        }
                    }
                }
            }
            // skip connection to the head
            let skip: &[f32] = match &l.head1x1 {
                Some(h) => {
                    h.apply(&l.z, &mut l.skip, n);
                    &l.skip
                }
                None => &l.z,
            };
            for r in 0..hin {
                for t in 0..n {
                    self.head_acc[r * CH + t] += skip[r * CH + t];
                }
            }
            // residual to the next layer
            match &l.layer1x1 {
                Some(d) => d.apply(&l.z, &mut l.next, n),
                None => {
                    let rows = self.chan.len() / CH;
                    l.next[..rows * CH].copy_from_slice(&l.z[..rows * CH]);
                }
            }
            for i in 0..l.next.len() {
                // rows beyond `n` are never read, so adding the whole stride is harmless
                l.next[i] += self.chan[i];
            }
            std::mem::swap(&mut self.chan, &mut l.next);
        }
        self.out_layers.copy_from_slice(&self.chan);
        self.head_hist.write(&self.head_acc, n);
        self.head.apply(&self.head_hist, &mut self.out_head, n);
        self.head_hist.advance(n);
    }

    fn reach(&self) -> usize {
        self.layers.iter().map(|l| l.conv.reach()).sum::<usize>() + self.head.reach()
    }
}

struct WaveNet {
    arrays: Vec<Array>,
    head_scale: f32,
    cond_rows: Vec<f32>,
}

fn usize_of(v: &Value, key: &str) -> Result<usize, String> {
    v.get(key).and_then(Value::as_u64).map(|n| n as usize).ok_or_else(|| format!("missing {key}"))
}

fn flag(v: Option<&Value>, key: &str) -> bool {
    v.and_then(|o| o.get(key)).and_then(Value::as_bool).unwrap_or(false)
}

fn ints(v: &Value, key: &str) -> Result<Vec<usize>, String> {
    v.get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("missing {key}"))?
        .iter()
        .map(|x| x.as_u64().map(|n| n as usize).ok_or_else(|| format!("bad {key}")))
        .collect()
}

impl WaveNet {
    fn parse(config: &Value, weights: &[f32]) -> Result<WaveNet, String> {
        if !config.get("head").map_or(true, Value::is_null) {
            return Err("WaveNets with a post-stack head are not supported".into());
        }
        let layers_json = config.get("layers").and_then(Value::as_array).ok_or("no layers in the config")?;
        if layers_json.is_empty() {
            return Err("no layers in the config".into());
        }
        let mut wt = Weights { w: weights, at: 0 };
        let mut arrays: Vec<Array> = Vec::new();
        for (ai, lj) in layers_json.iter().enumerate() {
            let input_size = usize_of(lj, "input_size")?;
            let cond_size = usize_of(lj, "condition_size")?;
            let channels = usize_of(lj, "channels")?;
            let bottleneck = lj.get("bottleneck").and_then(Value::as_u64).map_or(channels, |n| n as usize);
            let dilations = ints(lj, "dilations")?;
            let n = dilations.len();
            if n == 0 {
                return Err("a layer array without layers".into());
            }
            let kernels = match lj.get("kernel_sizes") {
                Some(_) => ints(lj, "kernel_sizes")?,
                None => vec![usize_of(lj, "kernel_size")?; n],
            };
            if kernels.len() != n {
                return Err("kernel_sizes and dilations differ in length".into());
            }
            let acts: Vec<Act> = match lj.get("activation") {
                Some(Value::Array(a)) => a.iter().map(Act::parse).collect::<Result<_, _>>()?,
                Some(one) => vec![Act::parse(one)?; n],
                None => return Err("missing activation".into()),
            };
            if acts.len() != n {
                return Err("activation and dilations differ in length".into());
            }
            // gating: per-layer modes (new) or a single `gated` flag (old)
            let mut gates: Vec<Option<Act>> = vec![None; n];
            if let Some(modes) = lj.get("gating_mode").and_then(Value::as_array) {
                let sec = lj.get("secondary_activation").and_then(Value::as_array);
                for (i, m) in modes.iter().enumerate().take(n) {
                    match m.as_str().unwrap_or("none") {
                        "none" => {}
                        "gated" => {
                            let s = sec.and_then(|s| s.get(i)).filter(|v| !v.is_null());
                            gates[i] = Some(match s {
                                Some(v) => Act::parse(v)?,
                                None => Act::Sigmoid,
                            });
                        }
                        other => return Err(format!("gating mode {other} is not supported")),
                    }
                }
            } else if lj.get("gated").and_then(Value::as_bool).unwrap_or(false) {
                gates = vec![Some(Act::Sigmoid); n];
            }
            for film in [
                "conv_pre_film",
                "conv_post_film",
                "input_mixin_pre_film",
                "input_mixin_post_film",
                "activation_pre_film",
                "activation_post_film",
                "layer1x1_post_film",
                "head1x1_post_film",
            ] {
                if flag(lj.get(film), "active") {
                    return Err("models with FiLM layers are not supported".into());
                }
            }
            let (l1_on, l1_groups) = match lj.get("layer1x1") {
                Some(o) => (flag(Some(o), "active"), o.get("groups").and_then(Value::as_u64).unwrap_or(1) as usize),
                None => (true, 1),
            };
            if !l1_on && bottleneck != channels {
                return Err("layer1x1 is off but the bottleneck differs from the channels".into());
            }
            let (h1_on, h1_out, h1_groups) = match lj.get("head1x1") {
                Some(o) => (
                    flag(Some(o), "active"),
                    o.get("out_channels").and_then(Value::as_u64).unwrap_or(0) as usize,
                    o.get("groups").and_then(Value::as_u64).unwrap_or(1) as usize,
                ),
                None => (false, 0, 1),
            };
            let groups_in = lj.get("groups_input").and_then(Value::as_u64).unwrap_or(1) as usize;
            let groups_mix = lj.get("groups_input_mixin").and_then(Value::as_u64).unwrap_or(1) as usize;
            // the head ("rechannel") that closes the array
            let (head_size, head_k, head_bias, head_dil) = match lj.get("head") {
                Some(h) if h.is_object() => (
                    usize_of(h, "out_channels")?,
                    usize_of(h, "kernel_size")?,
                    flag(Some(h), "bias"),
                    h.get("head_dilation").and_then(Value::as_u64).unwrap_or(1) as usize,
                ),
                _ => (usize_of(lj, "head_size")?, 1, flag(Some(lj), "head_bias"), 1),
            };
            let head_in = if h1_on { h1_out } else { bottleneck };

            let rechannel = Dense::read(&mut wt, input_size, channels, false, 1)?;
            let mut layers = Vec::with_capacity(n);
            for i in 0..n {
                let zc = if gates[i].is_some() { 2 * bottleneck } else { bottleneck };
                let conv = Conv::read(&mut wt, channels, zc, kernels[i], dilations[i], true, groups_in)?;
                let mixin = Dense::read(&mut wt, cond_size, zc, false, groups_mix)?;
                let layer1x1 = if l1_on { Some(Dense::read(&mut wt, bottleneck, channels, true, l1_groups)?) } else { None };
                let head1x1 = if h1_on { Some(Dense::read(&mut wt, bottleneck, h1_out, true, h1_groups)?) } else { None };
                layers.push(Layer {
                    hist: Hist::new(channels, conv.reach()),
                    conv,
                    mixin,
                    layer1x1,
                    head1x1,
                    act: acts[i],
                    gate: gates[i],
                    bottleneck,
                    z: vec![0.0; zc * CH],
                    mix: vec![0.0; zc * CH],
                    skip: vec![0.0; head_in * CH],
                    next: vec![0.0; channels * CH],
                });
            }
            let head = Conv::read(&mut wt, head_in, head_size, head_k, head_dil, head_bias, 1)?;
            if ai > 0 && arrays[ai - 1].head_size != head_in {
                return Err("layer arrays do not chain (head size mismatch)".into());
            }
            arrays.push(Array {
                rechannel,
                layers,
                head_hist: Hist::new(head_in, head.reach()),
                head,
                head_acc: vec![0.0; head_in * CH],
                head_size,
                chan: vec![0.0; channels * CH],
                out_layers: vec![0.0; channels * CH],
                out_head: vec![0.0; head_size * CH],
            });
            if cond_size != 1 {
                return Err("models with a condition input other than the guitar are not supported".into());
            }
        }
        let head_scale = wt.take(1)?[0];
        if wt.at != weights.len() {
            return Err(format!("the file has {} weights, the architecture uses {}", weights.len(), wt.at));
        }
        if arrays[0].rechannel.cin != 1 {
            return Err("only mono models are supported".into());
        }
        if arrays.last().is_none_or(|a| a.head_size != 1) {
            return Err("only mono models are supported".into());
        }
        Ok(WaveNet { arrays, head_scale, cond_rows: vec![0.0; CH] })
    }

    #[inline(always)]
    fn process(&mut self, buf: &mut [f32]) {
        for chunk in buf.chunks_mut(CH) {
            let n = chunk.len();
            self.cond_rows[..n].copy_from_slice(chunk);
            let (first, rest) = self.arrays.split_first_mut().unwrap();
            first.step(&self.cond_rows, &self.cond_rows, None, n);
            let mut prev = first;
            for a in rest.iter_mut() {
                a.step(&prev.out_layers, &self.cond_rows, Some(&prev.out_head), n);
                prev = a;
            }
            for t in 0..n {
                chunk[t] = self.head_scale * prev.out_head[t];
            }
        }
    }

    fn reach(&self) -> usize {
        self.arrays.iter().map(Array::reach).sum()
    }
}

// ---------------------------------------------------------------- LSTM

struct Lstm {
    hidden: usize,
    layers: Vec<LstmLayer>,
    head_w: Vec<f32>,
    head_b: f32,
}

struct LstmLayer {
    input: usize,
    w: Vec<f32>, // [4*hidden][input + hidden]
    b: Vec<f32>,
    h: Vec<f32>,
    c: Vec<f32>,
    gates: Vec<f32>,
}

impl Lstm {
    fn parse(config: &Value, weights: &[f32]) -> Result<Lstm, String> {
        let n_layers = usize_of(config, "num_layers")?;
        let input = usize_of(config, "input_size")?;
        let hidden = usize_of(config, "hidden_size")?;
        if input != 1 {
            return Err("only mono models are supported".into());
        }
        let mut wt = Weights { w: weights, at: 0 };
        let mut layers = Vec::new();
        for i in 0..n_layers {
            let inp = if i == 0 { input } else { hidden };
            let w = wt.take(4 * hidden * (inp + hidden))?.to_vec();
            let b = wt.take(4 * hidden)?.to_vec();
            let h0 = wt.take(hidden)?.to_vec();
            let c0 = wt.take(hidden)?.to_vec();
            layers.push(LstmLayer { input: inp, w, b, h: h0, c: c0, gates: vec![0.0; 4 * hidden] });
        }
        let head_w = wt.take(hidden)?.to_vec();
        let head_b = wt.take(1)?[0];
        if wt.at != weights.len() {
            return Err(format!("the file has {} weights, the architecture uses {}", weights.len(), wt.at));
        }
        Ok(Lstm { hidden, layers, head_w, head_b })
    }

    fn tick(&mut self, x: f32) -> f32 {
        let hd = self.hidden;
        let mut input: Vec<f32> = vec![x];
        for l in self.layers.iter_mut() {
            let stride = l.input + hd;
            for g in 0..4 * hd {
                let row = &l.w[g * stride..(g + 1) * stride];
                let mut s = l.b[g];
                for (w, v) in row[..l.input].iter().zip(&input) {
                    s += w * v;
                }
                for (w, v) in row[l.input..].iter().zip(&l.h) {
                    s += w * v;
                }
                l.gates[g] = s;
            }
            // gate order: input, forget, cell, output
            for i in 0..hd {
                let ig = Act::Sigmoid.apply(l.gates[i]);
                let fg = Act::Sigmoid.apply(l.gates[hd + i]);
                let gg = l.gates[2 * hd + i].tanh();
                let og = Act::Sigmoid.apply(l.gates[3 * hd + i]);
                l.c[i] = fg * l.c[i] + ig * gg;
                l.h[i] = og * l.c[i].tanh();
            }
            input = l.h.clone();
        }
        let last = &self.layers[self.layers.len() - 1].h;
        self.head_b + self.head_w.iter().zip(last).map(|(a, b)| a * b).sum::<f32>()
    }
}

/// Flush-to-zero while alive: denormal numbers make this kind of recurrent maths many times slower.
#[cfg(target_arch = "x86_64")]
struct Ftz(u32);

#[cfg(target_arch = "x86_64")]
impl Ftz {
    #[allow(deprecated)]
    fn on() -> Ftz {
        use std::arch::x86_64::{_mm_getcsr, _mm_setcsr};
        // SAFETY: SSE is part of x86_64; only the FTZ and DAZ bits are changed.
        unsafe {
            let old = _mm_getcsr();
            _mm_setcsr(old | 0x8040);
            Ftz(old)
        }
    }
}

#[cfg(target_arch = "x86_64")]
impl Drop for Ftz {
    #[allow(deprecated)]
    fn drop(&mut self) {
        // SAFETY: restores the value read in `on`.
        unsafe { std::arch::x86_64::_mm_setcsr(self.0) }
    }
}

// ---------------------------------------------------------------- the model

enum Net {
    Wave(WaveNet),
    Lstm(Lstm),
}

/// A loaded model, ready to run one sample at a time.
pub struct Model {
    net: Net,
    pub sample_rate: f32,
    /// Loudness of the model's output for a reference signal, in dB, when the file says.
    pub loudness: Option<f32>,
    pub name: String,
}

impl std::fmt::Debug for Model {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Model({:?}, {} Hz)", self.name, self.sample_rate)
    }
}

fn floats(v: &Value) -> Result<Vec<f32>, String> {
    v.as_array()
        .ok_or("no weights in the file")?
        .iter()
        .map(|x| x.as_f64().map(|f| f as f32).ok_or_else(|| "a weight is not a number".to_string()))
        .collect()
}

impl Model {
    /// Load a `.nam` file. `lite` picks the smaller network of a slimmable file.
    pub fn load(path: &Path, lite: bool) -> Result<Model, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let root: Value = serde_json::from_str(&text).map_err(|e| format!("{}: not a .nam file ({e})", path.display()))?;
        Model::from_json(&root, lite)
    }

    pub fn from_json(root: &Value, lite: bool) -> Result<Model, String> {
        let arch = root.get("architecture").and_then(Value::as_str).ok_or("no architecture in the file")?;
        let top_rate = root.get("sample_rate").and_then(Value::as_f64);
        let top_loud = root.pointer("/metadata/loudness").and_then(Value::as_f64);
        let name = root.pointer("/metadata/name").and_then(Value::as_str).unwrap_or("").to_string();
        let mut model = if arch == "SlimmableContainer" {
            let subs = root.pointer("/config/submodels").and_then(Value::as_array).ok_or("no submodels in the file")?;
            let mut subs: Vec<&Value> = subs.iter().collect();
            subs.sort_by(|a, b| {
                let key = |v: &Value| v.get("max_value").and_then(Value::as_f64).unwrap_or(1.0);
                key(a).total_cmp(&key(b))
            });
            let pick = if lite { subs.first() } else { subs.last() }.ok_or("no submodels in the file")?;
            let mut m = Model::from_json(pick.get("model").ok_or("a submodel without a model")?, lite)?;
            m.sample_rate = m.sample_rate.max(0.0);
            if m.name.is_empty() {
                m.name = name.clone();
            }
            m
        } else {
            let config = root.get("config").ok_or("no config in the file")?;
            let weights = floats(root.get("weights").ok_or("no weights in the file")?)?;
            let net = match arch {
                "WaveNet" => Net::Wave(WaveNet::parse(config, &weights)?),
                "LSTM" => Net::Lstm(Lstm::parse(config, &weights)?),
                other => return Err(format!("architecture {other} is not supported (WaveNet, LSTM and slimmable files are)")),
            };
            Model { net, sample_rate: 48000.0, loudness: None, name: name.clone() }
        };
        if let Some(r) = top_rate {
            model.sample_rate = r as f32;
        }
        if let Some(l) = top_loud {
            model.loudness = Some(l as f32);
        }
        if model.name.is_empty() {
            model.name = name;
        }
        model.warm_up();
        Ok(model)
    }

    /// Run silence through the network until it has settled (the biases give it a resting state).
    fn warm_up(&mut self) {
        let n = match &self.net {
            Net::Wave(w) => w.reach() + 1,
            Net::Lstm(_) => 2048,
        };
        let mut silence = vec![0.0f32; n];
        self.process(&mut silence);
    }

    #[inline(always)]
    fn run(&mut self, buf: &mut [f32]) {
        match &mut self.net {
            Net::Wave(w) => w.process(buf),
            Net::Lstm(l) => {
                for s in buf.iter_mut() {
                    *s = l.tick(*s);
                }
            }
        }
    }

    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2,fma")]
    unsafe fn run_avx2(&mut self, buf: &mut [f32]) {
        self.run(buf);
    }

    /// Run a block through the model in place (uses AVX2 and FMA when the CPU has them).
    pub fn process(&mut self, buf: &mut [f32]) {
        #[cfg(target_arch = "x86_64")]
        let _ftz = Ftz::on();
        #[cfg(target_arch = "x86_64")]
        if std::arch::is_x86_feature_detected!("avx2") && std::arch::is_x86_feature_detected!("fma") {
            // SAFETY: the features were just detected.
            unsafe { self.run_avx2(buf) };
            return;
        }
        self.run(buf);
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    /// A tiny legacy-format WaveNet: 1 array, 2 channels, 2 layers, kernel 2, dilations 1 and 2.
    pub(crate) fn tiny() -> (Value, usize) {
        let channels = 2;
        let layers = 2;
        // rechannel 1x2, per layer: conv 2 taps * 2*2 + bias 2, mixin 1x2, 1x1 2x2 + bias 2; head 2 + bias 1; scale 1
        let n = channels + layers * (2 * channels * channels + channels + channels + channels * channels + channels) + channels + 1 + 1;
        let weights: Vec<f32> = (0..n).map(|i| ((i * 37 % 19) as f32 - 9.0) / 25.0).collect();
        let v = json!({
            "version": "0.5.4",
            "architecture": "WaveNet",
            "config": {
                "layers": [{
                    "input_size": 1, "condition_size": 1, "head_size": 1, "channels": channels,
                    "kernel_size": 2, "dilations": [1, 2], "activation": "Tanh", "gated": false, "head_bias": true
                }],
                "head": null, "head_scale": 0.5
            },
            "weights": weights,
            "sample_rate": 48000
        });
        (v, n)
    }

    #[test]
    fn legacy_wavenet_consumes_exactly_its_weights() {
        let (v, _) = tiny();
        let mut m = Model::from_json(&v, false).expect("loads");
        let mut out: Vec<f32> = (0..64).map(|i| (i as f32 * 0.3).sin() * 0.5).collect();
        m.process(&mut out);
        assert!(out.iter().all(|x| x.is_finite()));
        assert!(out.iter().any(|x| x.abs() > 1e-4));
    }

    #[test]
    fn wrong_weight_count_is_rejected() {
        let (mut v, n) = tiny();
        v["weights"] = json!(vec![0.1f32; n + 3]);
        assert!(Model::from_json(&v, false).is_err());
        v["weights"] = json!(vec![0.1f32; n - 3]);
        assert!(Model::from_json(&v, false).is_err());
    }

    #[test]
    fn model_is_causal_and_time_invariant_after_warmup() {
        let (v, _) = tiny();
        let mut a = Model::from_json(&v, false).unwrap();
        let mut b = Model::from_json(&v, false).unwrap();
        let x: Vec<f32> = (0..3000).map(|i| ((i * 13 % 23) as f32 - 11.0) / 30.0).collect();
        let mut ya = x.clone();
        a.process(&mut ya);
        // same input in two chunks gives the same output (state carries over)
        let mut yb = Vec::new();
        for c in x.chunks(37) {
            let mut c = c.to_vec();
            b.process(&mut c);
            yb.extend(c);
        }
        assert_eq!(ya, yb);
    }

    #[test]
    fn unsupported_architecture_is_a_clean_error() {
        let v = json!({"architecture": "Mystery", "config": {}, "weights": []});
        assert!(Model::from_json(&v, false).unwrap_err().contains("not supported"));
    }
}
