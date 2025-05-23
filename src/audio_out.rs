use micromath::F32Ext;
use fixed::FixedU32;

use pio::program::pio_asm;
use pio::{ShiftConfig, ShiftDirection};

use core::f32::consts::TAU;
use log::error;

use super::*;

type Pio = pio::Pio<'static, PIO0>;
type StateMachine = pio::StateMachine<'static, PIO0, 0>;

const POINT_MAX: u32 = 2048;
const HALF_MAX: u32 = POINT_MAX / 2;
const FLT_HALF: f32 = HALF_MAX as f32;

static BUFFER: ConstStaticCell<[u32; 2048]> = ConstStaticCell::new([0; 2048]);

static PROGRESS: AtomicU64 = AtomicU64::new(0);

async fn audio_out_task(
    pio: Pio,
    sample_rate: f32,
    dma: peripherals::DMA_CH0,
) {
    let buffer_a = BUFFER_A.take();
    let buffer_b = BUFFER_B.take();
    let mut buffers = [buffer_a, buffer_b];

    let mut sm = init_pio(pio);
    let tx = sm.tx();

    let mut i = 0.0f32;
    let mut current_push = None;
    let sr_period = 1.0 / sample_rate;
    let mut dma = dma.into_ref();

    /*loop {
        for buffer in buffers.iter_mut() {
            let freq = 220.0;

            // fill buffer
            for point in buffer.iter_mut() {
                let sec_progress = i / 44100.0;
                let sample = sin(i, freq, 1.0, 1.0);
                *point = (FLT_HALF + sample * FLT_HALF) as u32;
                i += sr_period;
            }

            // wait for DMA starvation
            if let Some(transfer) = current_push.take() {
                transfer.await;
            }

            // swap buffers
            tx.dma_push(dma, buffer, false).await;

            // dma transfer complete, tell the
            // other task to begin its transfer
        }
    }*/
}

async fn buffer_task(
    init: bool,
    signal: SyncSignal,
    sample_rate: f32,
) {
    let freq = 220.0;

    let mut i = 0;

    if init {
        // tell the other task to begin its transfer
        signal.signal(u64::MAX);

        // recover progress from other task
        i = signal.wait().await;
    }

    loop {
        // fill buffer
        for point in buffer.iter_mut() {
            let progress = (i as f32) / sample_rate;
            let sample = sin(progress, freq, 1.0, 1.0);
            *point = (FLT_HALF + sample * FLT_HALF) as u32;
            i += 1;
        }

        // wait for other task to allow transfer
        if signal.wait().await != u64::MAX {
            error!("buffer_task: desync");
            continue;
        };

        // send progress
        signal.signal(i);

        // swap buffers
        tx.dma_push(dma, buffer, false).await;

        // dma transfer complete, tell the
        // other task to begin its transfer
        signal.signal(u64::MAX);

        // recover progress from other task
        i = signal.wait().await;
    }
}

fn init_pio(pio: Pio) -> StateMachine {
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
    cfg.use_program(&program, &[]);

    cfg.clock_divider = FixedU32::from_num(1);

    cfg.shift_in = ShiftConfig {
        auto_fill: true,
        threshold: 32,
        direction: ShiftDirection::Left,
    };

    cfg.shift_out = ShiftConfig {
        auto_fill: true,
        threshold: 32,
        direction: ShiftDirection::Right,
    };

    sm.set_config(&cfg);
    sm.set_enable(true);

    sm
}

fn sin(sec_progress: f32, freq: f32, dist_knee: f32, dist_factor: f32) -> f32 {
    let mut sample = F32Ext::sin(sec_progress * freq * TAU);

    if sample > dist_knee {
        sample -= dist_knee;
        sample *= dist_factor;
        sample += dist_knee;
    }

    if sample < -dist_knee {
        sample += dist_knee;
        sample *= dist_factor;
        sample -= dist_knee;
    }

    sample
}




