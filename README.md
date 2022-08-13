## pitch_shift

This crate has one library and one program inside.

### As a library

The library is a rust port of the code at https://github.com/cpuimage/pitchshift/.

It implementes the "Phase Vocoder" technique which shifts the pitch without stretching
the recording and without bringing in too many artifacts.

It exposes one type, `PitchShifter`, which allows you to shift the pitch of audio buffers.
It's up to you to bring the audio, maybe from a file or from your computer's microphone.

See https://docs.rs/pitch_shift for library usage instructions.

### As a program

The program at `examples/shift-wav.rs` allows you to shift the pitch of WAV files from your command line.

It can be installed this way:
```sh
cargo install pitch_shift --example shift-wav
```

Run it without any argument to learn how to use it.
