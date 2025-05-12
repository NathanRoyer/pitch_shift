use core::f32::consts::{PI, TAU};
use micromath::F32Ext;

const CROSSOVER: f32 = 0.25;
const RATIO: f32 = 1.0 / CROSSOVER;
const HALF_CROSSOVER: f32 = CROSSOVER * 0.5;
const LOWER_LIMIT: f32 = 0.5 - HALF_CROSSOVER;
const HIGHER_LIMIT: f32 = 0.5 + HALF_CROSSOVER;

const BUFSIZE: usize = 1024;
const BUFSIZE_F: f32 = BUFSIZE as f32;
const BUFSIZE_M1: usize = BUFSIZE - 1;
const BUFSIZE_M1F: f32 = BUFSIZE_M1 as f32;
const BUFSIZE_H: usize = BUFSIZE / 2;
const BUFSIZE_HF: f32 = BUFSIZE_H as f32;

#[inline(always)]
fn hamming(i: f32) -> f32 {
    0.5 - 0.5 * F32Ext::cos(i * TAU)
}

#[inline(always)]
fn window(i: f32) -> f32 {
    if i < LOWER_LIMIT {
        return 0.0;
    }

    if i > HIGHER_LIMIT {
        return 0.0;
    }

    hamming((i - LOWER_LIMIT) * RATIO)
}

#[inline(always)]
fn fade_out(i: f32) -> f32 {
    0.5 + 0.5 * F32Ext::cos(i * PI)
}

#[inline(always)]
fn fade_out_quick(i: f32) -> f32 {
    if i < LOWER_LIMIT {
        return 1.0;
    }

    if i > HIGHER_LIMIT {
        return 0.0;
    }

    fade_out((i - LOWER_LIMIT) * RATIO)
}

pub fn shift_frame(input: &[f32; BUFSIZE], output: &mut [f32; BUFSIZE], shift_semitones: f32) {
    let time_factor = 2.0_f32.powf(shift_semitones / 12.0);

    for dst in 0..BUFSIZE {
        let float_dst = dst as f32;

        let remaining = BUFSIZE_M1F - float_dst;
        let scaled_rem = remaining * time_factor;

        let scaled = float_dst * time_factor;
        let start = scaled < BUFSIZE_HF;

        let a_src = scaled % BUFSIZE_F;
        let b_src = (scaled - BUFSIZE_HF) % BUFSIZE_F;

        let a = interpolate(input, a_src);
        let b = interpolate(input, b_src);

        let mut a_factor = 1.0;

        if !start {
            a_factor = window(a_src / BUFSIZE_M1F);
        }

        let b_factor = 1.0 - a_factor;
        let mut out_sample = a * a_factor + b * b_factor;

        if scaled_rem < BUFSIZE_HF {
            let outro_src = BUFSIZE_M1F - scaled_rem;
            let outro = interpolate(input, outro_src);
            // `remaining` decreases, we're actually fading in
            let outro_factor = fade_out_quick(scaled_rem / BUFSIZE_HF);
            out_sample *= 1.0 - outro_factor;
            out_sample += outro * outro_factor;
        }

        output[dst] = out_sample;
    }
}

// linear sample interpolation
#[inline(always)]
fn interpolate(input: &[f32], index: f32) -> f32 {
    let h_factor = index.fract();
    let l_factor = 1.0 - h_factor;
    let l_index = index.trunc() as usize;
    let h_index = l_index + 1;

    let fallback = *input.last().unwrap();

    let low = input.get(l_index).copied().unwrap_or(fallback);
    let high = input.get(h_index).copied().unwrap_or(fallback);

    low * l_factor + high * h_factor
}
