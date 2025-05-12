use core::f32::consts::{PI, TAU};
use core::iter::Iterator;
use micromath::F32Ext;

// one 64th of 2048
const DIFF_LEN: usize = 32;
const DIFF_CENTER: usize = DIFF_LEN / 2;

#[inline(always)]
fn hamming(i: f32) -> f32 {
    0.5 - 0.5 * F32Ext::cos(i * TAU)
}

#[inline(always)]
fn bigger_hamming(i: f32) -> f32 {
    F32Ext::sqrt(hamming(i))
}

#[inline(always)]
fn fade_out(i: f32) -> f32 {
    0.5 + 0.5 * F32Ext::cos(i * PI)
}

pub fn shift_frame(
    input: &[f32; 2048],
    output: &mut [f32; 2048],
    shift_semitones: f32,
) {
    let time_factor = 2.0_f32.powf(shift_semitones / 12.0);

    let mut endbuf = [0.0f32; 2048];

    let scaled_size = 2048.0 / time_factor;
    let scaled_len = (scaled_size.trunc() as usize).min(2048);

    if scaled_len <= DIFF_LEN {
        // unacceptable
        return;
    }

    for a_dst in 0..scaled_len {
        let b_dst = 2047 - a_dst;
        let float_a_dst = a_dst as f32;

        let a_scaled = float_a_dst * time_factor;
        let b_scaled = 2047.0 - a_scaled;

        let a_src = a_scaled % 2048.0;
        let b_src = b_scaled % 2048.0;

        let a = interpolate(input, a_src);
        let b = interpolate(input, b_src);

        output[a_dst] = a;
        endbuf[b_dst] = b;
    }

    // 1. trouver la répétition la plus tardive

    let max_overlap = scaled_len - DIFF_LEN;
    let mut repeat_at = scaled_len;
    let mut lowest_diff = f32::INFINITY;

    for overlap in DIFF_LEN..max_overlap {
        let room = scaled_len - overlap;

        let a_iter = output[room..].windows(DIFF_LEN);
        let b_iter = output.windows(DIFF_LEN);
        let pair_iter = Iterator::zip(a_iter, b_iter);

        for (i, (a, b)) in pair_iter.enumerate() {
            let diff = centered_diff(a, b);

            if diff < lowest_diff {
                lowest_diff = diff;
                repeat_at = room + i + DIFF_CENTER;
            }
        }
    }

    // 2. opérer la répétion jusqu'à overlap l'endbuf
    // 3. trouver la meilleure transition finale
    // 4. opérer la transition finale
}

// linear sample interpolation
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

fn centered_diff(
    a: &[f32],
    b: &[f32],
) -> f32 {
    let center = DIFF_CENTER as f32;
    let mut diff = 0.0;

    for i in 0..DIFF_LEN {
        let distance = f32::abs((i as f32) - center) * 2.0; // arbitrary doubling
        diff += (a[i] - b[i]).abs() / (distance + 1.0);
    }

    diff
}
