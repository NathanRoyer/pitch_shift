// use num_traits::float::Float;
use micromath::F32Ext;

use microfft::inverse::ifft_1024;
use microfft::real::rfft_1024;
use microfft::Complex32;

use core::f32::consts::PI;
use core::f32::consts::TAU; // = 2xPI

const FRAME_SIZE: usize = 1024;
const HALF_FRAME_SIZE: usize = 512;
const FRAME_SIZE_F32: f32 = FRAME_SIZE as f32;

#[inline]
fn real_ifft(
    // only half of this must be written
    input: &mut [Complex32; FRAME_SIZE],
    output: &mut [f32; FRAME_SIZE],
) {
    let nyquist = input[0].im;
    input[0].im = 0.0;

    input[HALF_FRAME_SIZE] = Complex32::new(nyquist, 0.0);

    for i in 1..HALF_FRAME_SIZE {
        let j = HALF_FRAME_SIZE + i;
        let k = HALF_FRAME_SIZE - i;
        input[j] = input[k].conj();
    }

    let inversed = ifft_1024(input);

    for i in 0..FRAME_SIZE {
        output[i] = inversed[i].re;
    }
}

/// See [`PitchShifter::new`] & [`PitchShifter::shift_pitch`]
pub struct PitchShifter {
    fft_real: [f32; FRAME_SIZE],
    fft_cplx: [Complex32; FRAME_SIZE],

    in_fifo: [f32; FRAME_SIZE],
    out_fifo: [f32; FRAME_SIZE],

    last_phase: [f32; HALF_FRAME_SIZE],
    phase_sum: [f32; HALF_FRAME_SIZE],
    windowing: [f32; FRAME_SIZE],
    output_accumulator: [f32; FRAME_SIZE * 2],
    synthesized_frequency: [f32; FRAME_SIZE],
    synthesized_magnitude: [f32; FRAME_SIZE],

    overlap: usize,
    sample_rate: usize,
}

pub const SHIFTER_INIT: PitchShifter = PitchShifter {
    fft_real: [0.0; FRAME_SIZE],
    fft_cplx: [Complex32::ZERO; FRAME_SIZE],

    in_fifo: [0.0; FRAME_SIZE],
    out_fifo: [0.0; FRAME_SIZE],

    last_phase: [0.0; HALF_FRAME_SIZE],
    phase_sum: [0.0; HALF_FRAME_SIZE],
    windowing: [0.0; FRAME_SIZE],
    output_accumulator: [0.0; FRAME_SIZE * 2],
    synthesized_frequency: [0.0; FRAME_SIZE],
    synthesized_magnitude: [0.0; FRAME_SIZE],

    overlap: 0,
    sample_rate: 1,
};

impl PitchShifter {
    pub fn init(&mut self, sample_rate: usize) {
        // log::info!("init");
        self.sample_rate = sample_rate;

        for k in 0..FRAME_SIZE {
            let float_k = k as f32;
            self.windowing[k] = -0.5 * (float_k * TAU / FRAME_SIZE_F32).cos() + 0.5;
        }
        // log::info!("init done");
    }

    /// This is where the magic happens.
    ///
    /// The bigger `over_sampling`, the longer it will take to
    /// process, but the better the results. I put `16` in the
    /// `shift-wav` binary.
    ///
    /// `shift` is how many semitones to apply to the buffer.
    /// It is signed: a negative value will lower the tone and
    /// vice-versa.
    ///
    /// `in_b` is where the input buffer goes, and you must pass
    /// an output buffer of the same length in `out_b`.
    ///
    /// Note: It's actually not magic, sadly.
    pub async fn shift_pitch(&mut self, over_sampling: usize, shift: f32, in_b: &[f32; FRAME_SIZE], out_b: &mut [f32; FRAME_SIZE]) {
        let shift = 2.0_f32.powf(shift / 12.0);

        let step = FRAME_SIZE / over_sampling;
        let bin_frequencies = self.sample_rate as f32 / FRAME_SIZE_F32;
        let expected = TAU / (over_sampling as f32);
        let fifo_latency = FRAME_SIZE - step;
        let acc_oversamp: f32 = 2.0 / (HALF_FRAME_SIZE * over_sampling) as f32;

        if self.overlap == 0 {
            self.overlap = fifo_latency;
        }

        let pitch_weight = shift * bin_frequencies;
        let oversamp_weight = ((over_sampling as f32) / TAU) * pitch_weight;
        let mean_expected = expected / bin_frequencies;

        // log::info!("shift");

        for i in 0..FRAME_SIZE {
            // log::info!("i={i}");
            // log::info!("self.overlap = {}", self.overlap);
            // log::info!("fifo_latency = {}", fifo_latency);
            // super::Timer::after_millis(1).await;

            self.in_fifo[self.overlap] = in_b[i];
            out_b[i] = self.out_fifo[self.overlap - fifo_latency];
            self.overlap += 1;

            // log::info!("alright");
            // super::Timer::after_millis(1).await;

            if self.overlap >= FRAME_SIZE {
                self.overlap = fifo_latency;

                for k in 0..FRAME_SIZE {
                    self.fft_real[k] = self.in_fifo[k] * self.windowing[k];
                }

                // log::info!("windowing done");
                // super::Timer::after_millis(1).await;

                let fft_result = rfft_1024(&mut self.fft_real);
                self.fft_cplx[..HALF_FRAME_SIZE].copy_from_slice(fft_result);

                // log::info!("rfft_1024 done");
                // super::Timer::after_millis(1).await;

                self.synthesized_magnitude.fill(0.0);
                self.synthesized_frequency.fill(0.0);

                for k in 0..HALF_FRAME_SIZE {
                    let k_real = k as f32;
                    let index = (k_real * shift).round() as usize;
                    if index < HALF_FRAME_SIZE {
                        let (magnitude, phase) = to_polar(self.fft_cplx[k]);
                        let mut delta_phase = (phase - self.last_phase[k]) - k_real * expected;
                        // must not round here for some reason
                        let mut qpd = (delta_phase / PI) as i64;

                        if qpd >= 0 {
                            qpd += qpd & 1;
                        } else {
                            qpd -= qpd & 1;
                        }

                        delta_phase -= PI * qpd as f32;
                        self.last_phase[k] = phase;
                        self.synthesized_magnitude[index] += magnitude;
                        self.synthesized_frequency[index] = k_real * pitch_weight + oversamp_weight * delta_phase;
                    }
                }

                // log::info!("transpose done");
                // super::Timer::after_millis(1).await;

                // self.fft_cplx.fill(Complex32::ZERO);

                for k in 0..HALF_FRAME_SIZE {
                    self.phase_sum[k] += mean_expected * self.synthesized_frequency[k];

                    // todo lookup tables
                    let (sin, cos) = self.phase_sum[k].sin_cos();
                    let magnitude = self.synthesized_magnitude[k];

                    self.fft_cplx[k].im = sin * magnitude;
                    self.fft_cplx[k].re = cos * magnitude;
                }

                real_ifft(&mut self.fft_cplx, &mut self.fft_real);

                // log::info!("real_ifft done");
                // super::Timer::after_millis(1).await;

                for k in 0..FRAME_SIZE {
                    let product = self.windowing[k] * self.fft_real[k] * acc_oversamp;
                    self.output_accumulator[k] += product / 2.0;
                }

                self.out_fifo[..step].copy_from_slice(&self.output_accumulator[..step]);
                self.output_accumulator.copy_within(step..(step + FRAME_SIZE), 0);
                self.in_fifo.copy_within(step..(step + fifo_latency), 0);
            }
        }
    }
}

#[inline]
pub fn to_polar(cplx: Complex32) -> (f32, f32) {
    let norm = cplx.re.hypot(cplx.im);
    let arg = cplx.im.atan2(cplx.re);
    (norm, arg)
}
