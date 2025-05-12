use micromath::F32Ext;

use microfft::inverse::ifft_1024;
use microfft::real::rfft_1024;
use microfft::Complex32;

use core::f32::consts::{PI, TAU};
use core::mem::replace;

const HOP: usize = 32;
const HOP_F32: f32 = HOP as f32;

const FS: usize = 1024;
const HFS: usize = FS / 2;
const HFS_M1: usize = HFS - 1;
const FS_F32: f32 = FS as f32;
const FS_M1_F32: f32 = (FS - 1) as f32;

pub fn shift(input: &[f32], output: &mut [f32], shift_semitones: f32) {
    let shift_factor = 2.0_f32.powf(shift_semitones / 12.0); // 1.1225 pour +2st

    let mut buffer = [0.0f32; FS];
    let mut arg_ibuf = [0.0f32; HFS];
    let mut arg_obuf = [0.0f32; HFS];

    let len_f32 = input.len() as f32;
    let hops = (len_f32 / HOP_F32).ceil() as usize;

    for hop_index in 0..hops {
        let offset = hop_index * HOP;
        let slice = &input[offset..];
        let available = slice.len().min(FS);
        let mut win_sq_sum = 0.0;

        for i in 0..FS {
            let progress = (i as f32) / FS_M1_F32;
            let point = *slice.get(i).unwrap_or(&0.0);
            let windowing = hamming(progress);
            win_sq_sum += windowing * windowing;
            buffer[i] = point * windowing;
        }

        shift_frame(
            &mut buffer,
            &mut arg_ibuf,
            &mut arg_obuf,
            shift_factor,
            44100.0,
        );

        let slice = &mut output[offset..];
        for i in 0..available {
            let progress = (i as f32) / FS_M1_F32;
            let windowing = hamming(progress) * HOP_F32;
            slice[i] += buffer[i] * (windowing / win_sq_sum);
        }
    }
}

pub fn shift_frame(
    buffer: &mut [f32; FS],
    arg_ibuf: &mut [f32; HFS],
    arg_obuf: &mut [f32; HFS],
    shift_factor: f32,
    sample_rate: f32,
) {
    let mut tmp_cplx = [Complex32::ZERO; FS];
    let mut tmp_norm = [0.0f32; HFS];
    let mut tmp_freq = [0.0f32; HFS];

    let freq_inc = sample_rate / FS_F32;
    let phase_inc = TAU * HOP_F32 / FS_F32;
    let max_freq = sample_rate * 0.5;

    // FORWARD FFT

    let fft_result = rfft_1024(buffer);

    // ENCODE

    for i in 0..HFS {
        let (norm, arg) = to_polar(fft_result[i]);
        let prev_arg = replace(&mut arg_ibuf[i], arg);
        let delta = arg - prev_arg;

        let i_f32 = i as f32;
        let j = wrap_angle(delta - i_f32 * phase_inc) / phase_inc;
        let freq = (i_f32 + j) * freq_inc;

        // storing the frequency simplifies shifting
        tmp_norm[i] = norm;
        tmp_freq[i] = freq;
    }

    // SHIFT

    // let norm = linear_interpol(&tmp_norm, shift_factor);
    // let freq = linear_interpol(&tmp_freq, shift_factor).map(|f| f * shift_factor);

    for i in 0..HFS {
        let scaled = (i as f32) / shift_factor; // 1.1225
        let mut norm = interpolate(&tmp_norm, scaled);
        let freq = interpolate(&tmp_freq, scaled) * shift_factor; // 1.1225
        let invalid_freq = (freq <= 0.0) | (freq >= max_freq);

        if invalid_freq {
            norm = 0.0;
        }

        tmp_cplx[i] = Complex32::new(norm, freq);
    }

    // DECODE

    for i in 0..HFS {
        let norm = tmp_cplx[i].re;
        let freq = tmp_cplx[i].im;

        let i_f32 = i as f32;
        let j = (freq - i_f32 * freq_inc) / freq_inc;
        let delta = (i_f32 + j) * phase_inc;
        let arg = arg_obuf[i] + delta;
        arg_obuf[i] = wrap_angle(arg);

        let (im, re) = F32Ext::sin_cos(arg);
        tmp_cplx[i] = Complex32::new(re, im) * norm;
    }

    tmp_cplx[0] = Complex32::ZERO;
    tmp_cplx[HFS_M1] = Complex32::ZERO;

    // INVERSE FFT

    real_ifft(&mut tmp_cplx, buffer);
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

#[inline]
fn real_ifft(
    // only half of this must be written
    input: &mut [Complex32; FS],
    output: &mut [f32; FS],
) {
    let nyquist = input[0].im;
    input[0].im = 0.0;

    input[HFS] = Complex32::new(nyquist, 0.0);

    for i in 1..HFS {
        let j = HFS + i;
        let k = HFS - i;
        input[j] = input[k].conj();
    }

    let synthetized = ifft_1024(input);

    for i in 0..FS {
        output[i] = synthetized[i].re;
    }
}

#[inline(always)]
pub fn to_polar(cplx: Complex32) -> (f32, f32) {
    let norm = cplx.re.hypot(cplx.im);
    let arg = cplx.im.atan2(cplx.re);
    (norm, arg)
}

#[inline(always)]
fn wrap_angle(radians: f32) -> f32 {
    (radians + PI) % TAU - PI
}

#[inline(always)]
fn hamming(i: f32) -> f32 {
    0.5 - 0.5 * F32Ext::cos(i * TAU)
}

/*
fn linear_interpol(in_vec: [f32; HFS], factor: f32) -> [f32; HFS] {
    let mut output = [0.0; HFS]

    for i in 0..HFS {
        let scaled = (i as f32) / factor;

        let int_i = scaled as usize;
        let frac_i = scaled - (int_i as f32);

        if int_i >= HFS_M1 {
            continue;
        }

        let lower_point = in_vec[int_i];
        let higher_point = in_vec[int_i + 1];

        output[i] = frac_i * higher_point + (1.0 - frac_i) * lower_point;
    }

    output
}*/
