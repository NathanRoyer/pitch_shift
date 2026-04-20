#![doc = include_str!("../README.md")]
#![no_std]

use core::f32::consts::{PI, TAU, FRAC_1_PI};
use core::mem::replace;

use microfft::Complex32;
use micromath::F32Ext;

use microfft::inverse::ifft_1024 as microfft_ifft;
use microfft::real::rfft_1024 as microfft_rfft;
const FS: usize = 1024;

const IN_HOP: usize = 128;
const IN_HOP_F32: f32 = IN_HOP as f32;

const HFS: usize = FS / 2;
const DFS: usize = FS * 2;
const HFS_M1: usize = HFS - 1;
const FS_F32: f32 = FS as f32;
const FS_M1_F32: f32 = (FS - 1) as f32;

const IN_LAST_HOP: usize = FS - IN_HOP;

const HAMMING_FS_IN: [f32; FS] = const_hamming::gen_hamming_fs_in();
const HAMMING_FS_OUT: [f32; FS] = const_hamming::gen_hamming_fs_out();

type Half<T> = [T; HFS];
type Full<T> = [T; FS];

struct State<'a> {
    input: &'a mut Full<f32>,
    hammed: &'a mut Full<f32>,
    output: &'a mut Full<f32>,

    arg_ibuf: &'a mut Half<f32>,
    arg_obuf: &'a mut Half<f32>,

    tmp_cplx: &'a mut Full<Complex32>,
    tmp_norm: &'a mut Half<f32>,
    tmp_freq: &'a mut Half<f32>,
}

// sum of array lengths in State (unit = f32)
/// Total number of f32 needed for the shifter state
pub const TOTAL_F32: usize = FS + FS + FS + HFS + HFS + DFS + HFS + HFS;
/// Array of `TOTAL_F32` floats
pub type RawState = [f32; TOTAL_F32];

/// Pitch-shifting Interface
pub struct Shifter<C: AsMut<RawState>> {
    raw_state: C,
}

fn extract_n<const N: usize, T>(raw: &mut [T]) -> (&mut [T; N], &mut [T]) {
    let (extracted, remaining) = raw.split_at_mut(N);
    let extracted = extracted.try_into().expect("bad input slice length");
    (extracted, remaining)
}

fn cast<C: AsMut<RawState>>(shifter: &mut Shifter<C>) -> State<'_> {
    let raw = shifter.raw_state.as_mut();

    let (input, raw) = extract_n(raw);
    let (hammed, raw) = extract_n(raw);
    let (output, raw) = extract_n(raw);

    let (arg_ibuf, raw) = extract_n(raw);
    let (arg_obuf, raw) = extract_n(raw);

    let (tmp_cplx, raw) = extract_n::<DFS, f32>(raw);
    let (tmp_norm, raw) = extract_n(raw);
    let (tmp_freq, _) = extract_n(raw);

    let tmp_cplx = bytemuck::cast_slice_mut(tmp_cplx);
    let (tmp_cplx, _) = extract_n::<FS, Complex32>(tmp_cplx);

    State {
        input,
        hammed,
        output,

        arg_ibuf,
        arg_obuf,

        tmp_cplx,
        tmp_norm,
        tmp_freq,
    }
}

impl<C: AsMut<RawState>> Shifter<C> {
    /// Constructs a new Pitch Shifter around a float container.
    ///
    /// Usage if you have access to box/vec:
    ///
    /// ```rust
    /// type State = Box<[f32; TOTAL_F32]>;
    /// 
    /// fn new_shifter() -> Shifter<State> {
    ///     let state_vec = vec![0.0; TOTAL_F32];
    ///     let state_box: State = state_vec.try_into().unwrap();
    ///     Shifter::new(state_box)
    /// }
    /// ```
    pub fn new(mut container: C) -> Self {
        container.as_mut().fill(0.0);

        Self {
            raw_state: container,
        }
    }

    /// Shifts the pitch of 128 audio samples
    ///
    /// The speed factor is given by `128 / out_samples`. Use this to slow down or accelerate the recording.
    ///
    /// Panics if input is not 128-items long or if `out_samples` >= 1024.
    ///
    /// Returns a slice of `out_samples` audio samples (pitch-shifted).
    ///
    /// The heaviest operations of this method include:
    /// - many memory moves
    /// - many f32 multiplications
    /// - some fast trigonometry work
    /// - one inverse FFT
    /// - one forward FFT
    pub fn shift(
        &mut self,
        input: &[f32],
        shift_semitones: f32,
        out_samples: usize,
        sample_rate: f32,
    ) -> &[f32] {
        let shift_factor = 2.0_f32.powf(shift_semitones / 12.0);
        let out_hop_f32 = out_samples as f32;
        let out_last_hop = FS - out_samples;

        let State {
            input: history,
            hammed,
            output,
            arg_ibuf,
            arg_obuf,
            tmp_cplx,
            tmp_norm,
            tmp_freq,
        } = cast(self);

        // shift sample history one hop back
        history.copy_within(IN_HOP..FS, 0);

        // insert new sample data
        history[IN_LAST_HOP..].copy_from_slice(input);

        for i in 0..FS {
            hammed[i] = history[i] * HAMMING_FS_IN[i];
        }

        let synthetized = shift_frame(
            hammed,
            arg_ibuf,
            arg_obuf,
            tmp_cplx,
            tmp_norm,
            tmp_freq,
            shift_factor,
            out_hop_f32,
            sample_rate,
        );

        // shift sample history one hop back
        output.copy_within(out_samples..FS, 0);

        // insert new output data
        output[out_last_hop..].fill(0.0);

        for i in 0..FS {
            output[i] += synthetized[i].re * HAMMING_FS_OUT[i] * out_hop_f32;
        }

        &output[..out_samples]
    }
}

fn shift_frame<'a>(
    hammed: &mut Full<f32>,
    arg_ibuf: &mut Half<f32>,
    arg_obuf: &mut Half<f32>,
    tmp_cplx: &'a mut Full<Complex32>,
    tmp_norm: &mut Half<f32>,
    tmp_freq: &mut Half<f32>,
    shift_factor: f32,
    out_hop_f32: f32,
    sample_rate: f32,
) -> &'a mut [Complex32; FS] {
    const IN_PHASE_INC: f32 = IN_HOP_F32 * TAU / FS_F32;
    const INV_PHASE_INC: f32 = 1.0 / IN_PHASE_INC;
    const OUT_PHASE_FACTOR: f32 = TAU / FS_F32;

    let out_phase_inc = out_hop_f32 * OUT_PHASE_FACTOR;

    let max_freq = sample_rate * 0.5;
    let inv_shift_factor = 1.0 / shift_factor;

    // FORWARD FFT

    let fft_result = microfft_rfft(hammed);

    for i in 0..HFS {
        // ENCODE

        let (norm, arg) = to_polar(fft_result[i]);
        let prev_arg = replace(&mut arg_ibuf[i], arg);
        let delta = arg - prev_arg;

        let i_f32 = i as f32;
        let tmp = delta - i_f32 * IN_PHASE_INC;
        let j = wrap_angle(tmp) * INV_PHASE_INC;
        let freq = i_f32 + j;

        // storing the frequency simplifies shifting
        tmp_norm[i] = norm;
        tmp_freq[i] = freq;
    }

    for i in 0..HFS {
        // SHIFT

        let scaled = (i as f32) * inv_shift_factor;
        let mut norm = interpolate(&*tmp_norm, scaled);              // these two lines: 600ms
        let freq = interpolate(&*tmp_freq, scaled) * shift_factor;   //
        let invalid_freq = (freq <= 0.0) | (freq >= max_freq);

        if invalid_freq {
            norm = 0.0;
        }

        // DECODE

        let out_delta = freq * out_phase_inc;
        let arg = arg_obuf[i] + out_delta;
        arg_obuf[i] = wrap_angle(arg);

        let (im, re) = F32Ext::sin_cos(arg);
        tmp_cplx[i] = Complex32::new(re, im) * norm;
    }

    tmp_cplx[0] = Complex32::ZERO;
    tmp_cplx[HFS_M1] = Complex32::ZERO;

    // INVERSE FFT

    for i in 1..HFS {
        let j = HFS + i;
        let k = HFS - i;
        tmp_cplx[j] = tmp_cplx[k].conj();
    }

    microfft_ifft(tmp_cplx)
}

// linear signal interpolation
#[inline(always)]
fn interpolate(input: &[f32], index: f32) -> f32 {
    let h_factor = index.fract();
    let l_factor = 1.0 - h_factor;
    let l_index = index.trunc() as usize;
    let h_index = l_index + 1;

    let fallback = 0.0f32;

    let low = *input.get(l_index).unwrap_or(&fallback);
    let high = *input.get(h_index).unwrap_or(&fallback);

    low * l_factor + high * h_factor
}

#[inline(always)]
#[doc(hidden)]
pub fn to_polar(cplx: Complex32) -> (f32, f32) {
    let norm = cplx.re.hypot(cplx.im);
    let arg = cplx.im.atan2(cplx.re);
    (norm, arg)
}

#[inline(always)]
fn wrap_angle(radians: f32) -> f32 {
    (radians + PI).rem_euclid(TAU) - PI
}

mod const_hamming {
    use super::*;

    /// Returns the largest integer less than or equal to a number.
    // Stolen from micromath, turned const
    const fn floor(x: f32) -> f32 {
        let mut res = (x as i32) as f32;

        if x < res {
            res -= 1.0;
        }

        res
    }

    // Approximates `cos(x)` in radians with a maximum error of `0.002`.
    // Stolen from micromath, turned const
    const fn cos_approx(mut x: f32) -> f32 {
        x *= FRAC_1_PI / 2.0;
        x -= 0.25 + floor(x + 0.25);
        x *= 16.0 * (x.abs() - 0.5);
        x += 0.225 * x * (x.abs() - 1.0);
        x
    }

    #[inline(always)]
    const fn hamming(i: f32) -> f32 {
        0.5 - 0.5 * cos_approx(i * TAU)
    }

    pub const fn gen_hamming_fs_in() -> [f32; FS] {
        let mut out = [0.0; FS];

        let mut i = 0;
        while i < FS {
            out[i] = hamming(i as f32 / FS_M1_F32);
            i += 1;
        }

        out
    }

    pub const fn gen_hamming_fs_out() -> [f32; FS] {
        let hamming = gen_hamming_fs_in();
        let mut out = [0.0; FS];
        let mut sq_sum = 0.0;

        let mut i = 0;
        while i < FS {
            let tmp = hamming[i];
            sq_sum += tmp * tmp;
            i += 1;
        }

        let mut i = 0;
        while i < FS {
            out[i] = hamming[i] / sq_sum;
            i += 1;
        }

        out
    }
}
