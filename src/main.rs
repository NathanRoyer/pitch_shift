use hound::{WavReader, WavWriter, WavSpec, SampleFormat};
use core::f32::consts::TAU;
use std::time::Instant;

mod shift;

fn main() {
    println!("start");

    let scaler = i16::MAX as f32 + 0.5;

    let mut samples = Vec::new();
    let mut output_spec = WavSpec {
        channels: 1,
        sample_rate: 44100,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };

    if false {
        for i in 0..128_000 {
            let i = ((i as f32) / 44100.0) * (440.0 * TAU);
            samples.push(micromath::F32Ext::sin(i) * 8000.0);
        }
    } else {
        let input_path = "input.wav";

        let reader = WavReader::open(input_path).unwrap();
        let input_spec = reader.spec();

        let skip = input_spec.channels - 1;
        output_spec.sample_rate = input_spec.sample_rate;

        println!("input format: {:?}", input_spec.sample_format);
        println!("input bits: {:?}", input_spec.bits_per_sample);

        let mut iter = reader.into_samples::<i16>();

        while let Some(sample) = iter.next() {
            let point = sample.unwrap() as f32 + 0.5;
            samples.push(point / scaler);
            for _ in 0..skip {
                iter.next();
            }
        }
    }

    let mut writer = WavWriter::create("output.wav", output_spec).unwrap();
    let mut output = vec![0.0; samples.len()];
    let then = Instant::now();

    shift::shift(&samples, &mut output, 2.0);

    println!("elapsed: {}ms", then.elapsed().as_millis());

    for sample in output {
        writer.write_sample((sample * scaler - 0.5) as i16).unwrap();
    }

    writer.finalize().unwrap();
}

