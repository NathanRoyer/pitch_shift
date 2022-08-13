use rustfft::FftPlanner;
use realfft::RealToComplexEven;
use realfft::ComplexToRealEven;
use realfft::RealToComplex;
use realfft::ComplexToReal;
use realfft::num_complex::Complex;

use std::f32::consts::PI;
use std::f32::consts::TAU; // = 2xPI

type SampleReal = f32;
const COMPLEX_ZERO: Complex<SampleReal> = Complex::new(0.0, 0.0);

/// See [`PitchShifter::new`] & [`PitchShifter::shift_pitch`]
pub struct PitchShifter {
    forward_fft: RealToComplexEven<SampleReal>,
    inverse_fft: ComplexToRealEven<SampleReal>,
    ffft_scratch_len: usize,
    ifft_scratch_len: usize,
    fft_scratch: Vec<Complex<SampleReal>>,
    fft_real: Vec<SampleReal>,
    fft_cplx: Vec<Complex<SampleReal>>,

    in_fifo: Vec<SampleReal>,
    out_fifo: Vec<SampleReal>,

    last_phase: Vec<SampleReal>,
    phase_sum: Vec<SampleReal>,
    windowing: Vec<SampleReal>,
    output_accumulator: Vec<SampleReal>,
    synthesized_frequency: Vec<SampleReal>,
    synthesized_magnitude: Vec<SampleReal>,

    frame_size: usize,
    overlap: usize,
    sample_rate: usize,
}

impl PitchShifter {
    /// Phase Vocoding works by extracting overlapping windows
    /// from a buffer and processing them individually before
    /// merging the results into the output buffer.
    ///
    /// You must set a duration in miliseconds for these windows;
    /// 50ms is a good value.
    ///
    /// The sample rate argument must correspond to the sample
    /// rate of the buffer(s) you will provide to
    /// [`PitchShifter::shift_pitch`], which is how many values
    /// correspond to one second of audio in the buffer.
    pub fn new(window_duration_ms: usize, sample_rate: usize) -> Self {
        let mut frame_size = sample_rate * window_duration_ms / 1000;
        frame_size += frame_size % 2;
        let fs_real = frame_size as SampleReal;

        let double_frame_size = frame_size * 2;
        let half_frame_size = (frame_size / 2) + 1;

        let mut planner = FftPlanner::new();
        let forward_fft = RealToComplexEven::new(frame_size, &mut planner);
        let inverse_fft = ComplexToRealEven::new(frame_size, &mut planner);
        let ffft_scratch_len = forward_fft.get_scratch_len();
        let ifft_scratch_len = inverse_fft.get_scratch_len();
        let scratch_len = ffft_scratch_len.max(ifft_scratch_len);

        let mut windowing = vec![0.0; frame_size];
        for k in 0..frame_size {
            windowing[k] = -0.5 * (TAU * (k as SampleReal) / fs_real).cos() + 0.5;
        }

        Self {
            forward_fft,
            inverse_fft,
            ffft_scratch_len,
            ifft_scratch_len,
            fft_scratch: vec![COMPLEX_ZERO; scratch_len],
            fft_real: vec![0.0; frame_size],
            fft_cplx: vec![COMPLEX_ZERO; half_frame_size],

            in_fifo: vec![0.0; frame_size],
            out_fifo: vec![0.0; frame_size],

            last_phase: vec![0.0; half_frame_size],
            phase_sum: vec![0.0; half_frame_size],
            windowing,
            output_accumulator: vec![0.0; double_frame_size],
            synthesized_frequency: vec![0.0; frame_size],
            synthesized_magnitude: vec![0.0; frame_size],

            frame_size,
            overlap: 0,
            sample_rate,
        }
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
    pub fn shift_pitch(&mut self, over_sampling: usize, shift: SampleReal, in_b: &[SampleReal], out_b: &mut [SampleReal]) {
        let shift = 2.0_f32.powf(shift / 12.0);
        let fs_real = self.frame_size as SampleReal;
        let half_frame_size = (self.frame_size / 2) + 1;

        let step = self.frame_size / over_sampling;
        let bin_frequencies = self.sample_rate as SampleReal / fs_real;
        let expected = TAU / (over_sampling as SampleReal);
        let fifo_latency = self.frame_size - step;

        if self.overlap == 0 {
            self.overlap = fifo_latency;
        }

        let pitch_weight = shift * bin_frequencies;
        let oversamp_weight = ((over_sampling as SampleReal) / TAU) * pitch_weight;
        let mean_expected = expected / bin_frequencies;

        for i in 0..out_b.len() {
            self.in_fifo[self.overlap] = in_b[i];
            out_b[i] = self.out_fifo[self.overlap - fifo_latency];
            self.overlap += 1;
            if self.overlap >= self.frame_size {
                self.overlap = fifo_latency;

                for k in 0..self.frame_size {
                    self.fft_real[k] = self.in_fifo[k] * self.windowing[k];
                }

                let _ = self.forward_fft.process_with_scratch(
                    &mut self.fft_real,
                    &mut self.fft_cplx,
                    &mut self.fft_scratch[..self.ffft_scratch_len],
                );//.unwrap();

                self.synthesized_magnitude.fill(0.0);
                self.synthesized_frequency.fill(0.0);

                for k in 0..half_frame_size {
                    let k_real = k as SampleReal;
                    let index = (k_real * shift).round() as usize;
                    if index < half_frame_size {
                        let (magnitude, phase) = self.fft_cplx[k].to_polar();
                        let mut delta_phase = (phase - self.last_phase[k]) - k_real * expected;
                        // must not round here for some reason
                        let mut qpd = (delta_phase / PI) as i64;

                        if qpd >= 0 {
                            qpd += qpd & 1;
                        } else {
                            qpd -= qpd & 1;
                        }

                        delta_phase -= PI * qpd as SampleReal;
                        self.last_phase[k] = phase;
                        self.synthesized_magnitude[index] += magnitude;
                        self.synthesized_frequency[index] = k_real * pitch_weight + oversamp_weight * delta_phase;
                    }
                }

                self.fft_cplx.fill(COMPLEX_ZERO);

                for k in 0..half_frame_size {
                    self.phase_sum[k] += mean_expected * self.synthesized_frequency[k];

                    let (sin, cos) = self.phase_sum[k].sin_cos();
                    let magnitude = self.synthesized_magnitude[k];

                    self.fft_cplx[k].im = sin * magnitude;
                    self.fft_cplx[k].re = cos * magnitude;
                }

                let _ = self.inverse_fft.process_with_scratch(
                    &mut self.fft_cplx,
                    &mut self.fft_real,
                    &mut self.fft_scratch[..self.ifft_scratch_len],
                );//.unwrap();

                let acc_oversamp: SampleReal = 2.0 / (half_frame_size * over_sampling) as SampleReal;

                for k in 0..self.frame_size {
                    let product = self.windowing[k] * self.fft_real[k] * acc_oversamp;
                    self.output_accumulator[k] += product / 2.0;
                }

                self.out_fifo[..step].copy_from_slice(&self.output_accumulator[..step]);
                self.output_accumulator.copy_within(step..(step + self.frame_size), 0);
                self.in_fifo.copy_within(step..(step + fifo_latency), 0);
            }
        }
    }
}