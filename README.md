## pitch_shift

This crate contains one library and one program.

### As a library

The library was initially a port of <https://github.com/cpuimage/pitchshift/>.

It implementes the "Phase Vocoder" technique which shifts the pitch without stretching
the recording and without bringing in too many artifacts (though some are still present).

Using the `out_samples` parameter, users can also slow down or accelerate the sound (without altering the pitch).

It exposes one type, `Shifter`, which allows you to shift the pitch of audio buffers.
It's up to you to bring the audio (128 samples at a time), maybe from a file or from your computer's microphone.
Its latency is fixed to `1024 - out_samples` samples.

See <https://docs.rs/pitch_shift> for documentation.

### As a program

The program at `examples/shift-wav.rs` allows you to shift the pitch of WAV files from your command line.

It can be installed this way:
```sh
cargo install pitch_shift --example shift-wav
```

Run it without any argument to learn how to use it.
