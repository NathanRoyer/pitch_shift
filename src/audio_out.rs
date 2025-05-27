use micromath::F32Ext;
use fixed::FixedU32;

use pio::program::{pio_asm, InstructionOperands, OutDestination};
use pio::{ShiftConfig, ShiftDirection, Direction};

use core::f32::consts::TAU;
use core::array::from_fn;

use super::*;

type Pio = pio::Pio<'static, PIO0>;
type StateMachine = pio::StateMachine<'static, PIO0, 0>;

const HALF_MAX: u32 = RESOLUTION / 2;
const FLT_HALF: f32 = HALF_MAX as f32;

static BUFFER_A: Mutex<[u32; 1024]> = Mutex::new([0; 1024]);
static BUFFER_B: Mutex<[u32; 1024]> = Mutex::new([0; 1024]);

#[embassy_executor::task]
pub async fn audio_gen_task() {
    let sample_progress = 1.0 / (SAMPLE_RATE as f32);
    let mut toggle = false;
    let mut phases = [0.0; 64];
    let shifts: [f32; 64] = from_fn(|i| 2.0f32.powf(i as f32 / 12.0));

    loop {
        sleep_us(1).await;

        let buffer = match toggle {
            false => &BUFFER_A,
            true => &BUFFER_B,
        };

        let mut handle = buffer.lock().await;
        toggle = !toggle;

        for point in handle.iter_mut() {
            let mut tmp = 0.0;
            let mut num = 0.0f32;

            let mut l = NOTES_L.load(Ordering::Relaxed);
            let mut h = NOTES_H.load(Ordering::Relaxed);

            let a = FADER_A.load(Ordering::Relaxed) as f32;
            let b = FADER_B.load(Ordering::Relaxed) as f32;

            let dist_k = a / 128.0;
            let dist_f = b / 128.0;

            for mut i in 0..32 {
                let phase = &mut phases[i];
                let shift = shifts[i];

                if (l & 1) != 0 {
                    let freq = 110.0 * shift;
                    *phase += freq * sample_progress;

                    tmp += F32Ext::sin(*phase * TAU);
                    num += 1.0;
                }

                i += 32;
                let phase = &mut phases[i];
                let shift = shifts[i];

                if (h & 1) != 0 {
                    let freq = 110.0 * shift;
                    *phase += freq * sample_progress;

                    tmp += F32Ext::sin(*phase * TAU);
                    num += 1.0;
                }

                l >>= 1;
                h >>= 1;
            }

            tmp /= num.max(1.0);
            // tmp *= 0.4;
            // tmp = dist(tmp, dist_k, dist_f).clamp(-1.0, 1.0);
            *point = (FLT_HALF + tmp * FLT_HALF) as u32;
        }
    }
}

#[embassy_executor::task]
pub async fn dma_forward_task(
    pio: Pio,
    dma: peripherals::DMA_CH0,
    pin: peripherals::PIN_22,
) {
    let mut sm = init_pio(pio, pin);
    let tx = sm.tx();

    let mut dma = dma.into_ref();
    let mut toggle = false;

    loop {
        sleep_us(1).await;

        let buffer = match toggle {
            false => &BUFFER_A,
            true => &BUFFER_B,
        };

        let handle = buffer.lock().await;
        toggle = !toggle;

        tx.dma_push(dma.reborrow(), &*handle, false).await;
    }
}

fn init_pio(pio: Pio, pin: peripherals::PIN_22) -> StateMachine {
    let Pio {
        mut common,
        sm0: mut sm,
        ..
    } = pio;

    // takes two ticks to make 1/2048th of a sample
    let code = pio_asm!(
        ".side_set 1 opt"
            "pull noblock    side 0"
            "mov x, osr"
            "mov y, isr"
        "countloop:"
            "jmp x!=y noset"
            "jmp skip        side 1"
        "noset:"
            "nop"
        "skip:"
            "jmp y-- countloop"
    );

    let program = common.load_program(&code.program);
    let mut cfg = pio::Config::default();
    sm.set_enable(false);

    let pin = common.make_pio_pin(pin);
    cfg.use_program(&program, &[&pin]);
    cfg.clock_divider = FixedU32::from_num(1);

    cfg.shift_in = ShiftConfig {
        auto_fill: true,
        threshold: 32,
        direction: ShiftDirection::Left,
    };

    sm.set_config(&cfg);
    sm.set_pin_dirs(Direction::Out, &[&pin]);

    sm.tx().push(RESOLUTION);
    unsafe {
        sm.exec_instr(
            InstructionOperands::PULL {
                if_empty: false,
                block: false,
            }
            .encode(),
        );
        sm.exec_instr(
            InstructionOperands::OUT {
                destination: OutDestination::ISR,
                bit_count: 32,
            }
            .encode(),
        );
    };

    sm.tx().push(512);
    sm.set_enable(true);

    sm
}

fn dist(mut sample: f32, knee: f32, factor: f32) -> f32 {
    if sample > knee {
        sample -= knee;
        sample *= factor;
        sample += knee;
    }

    if sample < -knee {
        sample += knee;
        sample *= factor;
        sample -= knee;
    }

    sample
}




