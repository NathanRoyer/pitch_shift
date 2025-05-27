use micromath::F32Ext;

use super::*;

const I2C_ADDR: u16 = 0x20;

const REG_GPIOA: u8 = 0x12;
// const REG_GPIOB: u8 = 0x13;

const REG_IODIRA: u8 = 0x00;
// const REG_IODIRB: u8 = 0x01;

type I2c = i2c::I2c<'static, I2C0, i2c::Async>;
type NoteVec = ArrayVec::<u8, 32>;
type ButtonVec = ArrayVec::<Button, 32>;

#[derive(Copy, Clone, Debug)]
enum Button {
    Sustain,   // menu_2  + 8
    Vibrato,   // menu_2  + 9
    Start,     // menu_2  + 10
    Sync,      // menu_2  + 11
    Kick,      // menu_2  + 12
    Cymbal,    // menu_2  + 13
    Snare,     // menu_2  + 14
    PlaySpace, // menu_2  + 15

    Mandolin,  // menu_14 + 8
    Piano,     // menu_14 + 9
    Harp,      // menu_14 + 10
    Guitar,    // menu_14 + 11
    Clarinet,  // menu_14 + 12
    Oboe,      // menu_14 + 13
    Violin,    // menu_14 + 14
    Xylo,      // menu_14 + 15

    Disco,     // menu_15 + 8
    Pops,      // menu_15 + 9
    Rhumba,    // menu_15 + 10
    Beat16,    // menu_15 + 11
    Waltz,     // menu_15 + 12
    March,     // menu_15 + 13
    Rock,      // menu_15 + 14
    Bossa,     // menu_15 + 15

    TempoM,    // menu_16 + 8
    TempoP,    // menu_16 + 9
    Accomp,    // menu_16 + 10
    Stop,      // menu_16 + 11
    Demo,      // menu_16 + 12
    Record,    // menu_16 + 13
    PlayStop,  // menu_16 + 14
    Program,   // menu_16 + 15
}

mod buttons {
    use super::Button::{self, *};

    pub const MENU_2: [Button; 8] = [Sustain, Vibrato, Start, Sync, Kick, Cymbal, Snare, PlaySpace];
    pub const MENU_14: [Button; 8] = [Mandolin, Piano, Harp, Guitar, Clarinet, Oboe, Violin, Xylo];
    pub const MENU_15: [Button; 8] = [Disco, Pops, Rhumba, Beat16, Waltz, March, Rock, Bossa];
    pub const MENU_16: [Button; 8] = [TempoP, TempoM, Accomp, Stop, Demo, Record, PlayStop, Program];
}

use buttons::{MENU_2, MENU_14, MENU_15, MENU_16};

pub struct KeyboardMap {
    pub menu_2: Input<'static>, // PIN_20
    pub menu_14: Input<'static>, // PIN_9
    pub menu_15: Input<'static>, // PIN_8
    pub menu_16: Input<'static>, // PIN_7

    pub note_1: Input<'static>, // PIN_21
    pub note_2: Input<'static>, // PIN_6
    pub note_3: Input<'static>, // PIN_10
    pub note_4: Input<'static>, // PIN_15
    pub note_5: Input<'static>, // PIN_14

    pub adc_p1: Channel<'static>, // PIN_40
    pub adc_p2: Channel<'static>, // PIN_41
    pub adc: Adc<'static, adc::Async>,
}

async fn write_word(bus: &mut I2c, reg: u8, word: u16) {
    let lsb = (word >> 0) as u8;
    let msb = (word >> 8) as u8;

    match bus.write_async(I2C_ADDR, [reg, lsb, msb]).await {
        Err(msg) => log::error!("I2C WRITE: {msg:?}"),
        Ok(()) => (),
    };
}

#[embassy_executor::task]
pub async fn kb_test(
    i2c0: I2C0,
    scl: peripherals::PIN_17,
    sda: peripherals::PIN_16,
    mut kb_map: KeyboardMap,
) {
    let config = i2c::Config::default();
    let mut bus = I2c::new_async(i2c0, scl, sda, Irqs, config);
    let bus = &mut bus;

    let mut notes = NoteVec::new();
    let mut buttons = ButtonVec::new();

    // give everyone a break for once
    sleep_ms(100).await;

    // make everything an output
    write_word(bus, REG_IODIRA, 0).await;

    loop {
        let a = adc_map(kb_map.adc.read(&mut kb_map.adc_p1).await.ok(), 3200, 4076);
        let b = adc_map(kb_map.adc.read(&mut kb_map.adc_p2).await.ok(), 3140, 4076);

        FADER_A.store((a * 255.0) as u8, Ordering::Relaxed);
        FADER_B.store((b * 255.0) as u8, Ordering::Relaxed);

        for i in 0..8 {
            keyboard_pass(&kb_map, bus, i, &mut notes, &mut buttons).await;
        }

        let mut l = 0u32;
        let mut h = 0u32;

        for note in &notes {
            let (dst, i) = match *note < 32 {
                true => (&mut l, *note),
                false => (&mut h, note - 32),
            };

            *dst |= 1 << i;
        }

        NOTES_L.store(l, Ordering::Relaxed);
        NOTES_H.store(h, Ordering::Relaxed);

        log::info!("{a:.2} | {b:.2} | {:?} {:?}", notes, buttons);
        notes.clear();
        buttons.clear();
        sleep_us(10).await;
    }
}

async fn keyboard_pass(
    kb_map: &KeyboardMap,
    bus: &mut I2c,
    index: usize,
    notes: &mut NoteVec,
    buttons: &mut ButtonVec,
) {
    // turn the selected output on
    let mask = 0x101 << (index as u16);
    write_word(bus, REG_GPIOA, mask).await;

    sleep_us(100).await;

    // read inputs

    if kb_map.menu_2.is_high() { buttons.push(MENU_2[index]); }
    if kb_map.menu_14.is_high() { buttons.push(MENU_14[index]); }
    if kb_map.menu_15.is_high() { buttons.push(MENU_15[index]); }
    if kb_map.menu_16.is_high() { buttons.push(MENU_16[index]); }

    let output_to_st_offset = [4, 5, 6, 7, 0, 1, 2, 3];
    let st_offset = output_to_st_offset[index];

    if kb_map.note_2.is_high() { notes.push(0 + st_offset); }
    if kb_map.note_3.is_high() { notes.push(8 + st_offset); }
    if kb_map.note_4.is_high() { notes.push(16 + st_offset); }
    if kb_map.note_5.is_high() { notes.push(24 + st_offset); }
    if kb_map.note_1.is_high() { notes.push(32 + st_offset); }
}

fn adc_map(x: Option<u16>, min: u16, max: u16) -> f32 {
    let scaled_max = (max - min) as f32;
    let x = x.unwrap_or(min).saturating_sub(min) as f32;
    let on_one = 1.0 - (x / scaled_max);
    let x = (on_one / 1.78).powi(4) * 10.0;
    x
}
