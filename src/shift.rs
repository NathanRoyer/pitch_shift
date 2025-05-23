use micromath::F32Ext;

use microfft::inverse::ifft_1024;
use microfft::real::rfft_1024;
use microfft::Complex32;

use static_cell::ConstStaticCell;

use core::f32::consts::{PI, TAU, FRAC_1_PI};
use core::mem::replace;

const HOP: usize = 200;
const HOP_F32: f32 = HOP as f32;

const FS: usize = 1024;
const HFS: usize = FS / 2;
const HFS_M1: usize = HFS - 1;
const FS_F32: f32 = FS as f32;
const FS_M1_F32: f32 = (FS - 1) as f32;

const ZERO_F32_FS: [f32; FS] = [0.0; FS];
const ZERO_F32_HFS: [f32; HFS] = [0.0; HFS];
const ZERO_CPLX_FS: [Complex32; FS] = [Complex32::ZERO; FS];

static SAMPLES_BUFFER: ConstStaticCell<[f32; FS]> = ConstStaticCell::new(ZERO_F32_FS);
static ARG_IBUF: ConstStaticCell<[f32; HFS]> = ConstStaticCell::new(ZERO_F32_HFS);
static ARG_OBUF: ConstStaticCell<[f32; HFS]> = ConstStaticCell::new(ZERO_F32_HFS);

static TMP_CPLX: ConstStaticCell<[Complex32; FS]> = ConstStaticCell::new(ZERO_CPLX_FS);
static TMP_NORM: ConstStaticCell<[f32; HFS]> = ConstStaticCell::new(ZERO_F32_HFS);
static TMP_FREQ: ConstStaticCell<[f32; HFS]> = ConstStaticCell::new(ZERO_F32_HFS);

const HAMMING_FS_IN: [f32; FS] = const_hamming::gen_hamming_fs_in();
const HAMMING_FS_OUT: [f32; FS] = const_hamming::gen_hamming_fs_out();

pub fn shift(
    input: &[f32],
    output: &mut [f32],
    shift_semitones: f32,
    sample_rate: f32,
) {
    let shift_factor = 2.0_f32.powf(shift_semitones / 12.0); // 1.1225 pour +2st

    let buffer = SAMPLES_BUFFER.take();
    let arg_ibuf = ARG_IBUF.take();
    let arg_obuf = ARG_OBUF.take();

    let tmp_cplx = TMP_CPLX.take();
    let tmp_norm = TMP_NORM.take();
    let tmp_freq = TMP_FREQ.take();

    let len_f32 = input.len() as f32;
    let hops = (len_f32 / HOP_F32).ceil() as usize;

    log::info!("hops = {hops}");

    for hop_index in 0..hops {
        let offset = hop_index * HOP;
        let slice = &input[offset..];
        let available = slice.len().min(FS);

        for i in 0..FS {
            let point = *slice.get(i).unwrap_or(&0.0);
            buffer[i] = point * HAMMING_FS_IN[i];
        }

        let synthetized = shift_frame(
            buffer,
            arg_ibuf,
            arg_obuf,
            tmp_cplx,
            tmp_norm,
            tmp_freq,
            shift_factor,
            sample_rate,
        );

        let slice = &mut output[offset..];
        for i in 0..available {
            slice[i] += synthetized[i].re * HAMMING_FS_OUT[i];
        }
    }
}

pub fn shift_frame<'a>(
    // samples buffer
    buffer: &mut [f32; FS],
    // inout buffers
    arg_ibuf: &mut [f32; HFS],
    arg_obuf: &mut [f32; HFS],
    // work buffers
    tmp_cplx: &'a mut [Complex32; FS],
    tmp_norm: &mut [f32; HFS],
    tmp_freq: &mut [f32; HFS],
    // settings
    shift_factor: f32,
    sample_rate: f32,
) -> &'a mut [Complex32; FS] {
    const PHASE_INC: f32 = TAU * HOP_F32 / FS_F32;
    const INV_PHASE_INC: f32 = 1.0 / PHASE_INC;

    let max_freq = sample_rate * 0.5;
    let inv_shift_factor = 1.0 / shift_factor;

    // FORWARD FFT

    // 1500ms
    let fft_result = rfft_1024(buffer);

    for i in 0..HFS {
        // ENCODE

        let (norm, arg) = to_polar(fft_result[i]);
        let prev_arg = replace(&mut arg_ibuf[i], arg);
        let delta = arg - prev_arg;

        let i_f32 = i as f32;
        let tmp = delta - i_f32 * PHASE_INC;
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

        let delta = freq * PHASE_INC;
        let arg = arg_obuf[i] + delta;
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

    // 4000ms
    ifft_1024(tmp_cplx)
}

// 1379 hops
// 8889ms

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
            out[i] = (hamming[i] * HOP_F32) / sq_sum;
            i += 1;
        }

        out
    }
}
