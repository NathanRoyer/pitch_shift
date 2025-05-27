//! This example shows how to use USB (Universal Serial Bus) in the RP2040 chip.
//!
//! This creates a USB serial port that echos.

#![no_std]
#![no_main]

use embassy_time::Timer;
use embassy_sync::mutex;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_executor::Executor;

use embassy_rp::Peripheral;
use embassy_rp::clocks::{rosc_freq, xosc_freq, clk_sys_freq};
use embassy_rp::multicore::{spawn_core1, Stack};
use embassy_rp::peripherals::{self, USB, I2C0, PIO0};
use embassy_rp::gpio::{Level, Output, Input, Pull};
use embassy_rp::adc::{self, Adc, Channel};
use embassy_rp::bind_interrupts;
use embassy_rp::config::Config;
use embassy_rp::i2c;
use embassy_rp::usb;
use embassy_rp::pio;

use core::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use static_cell::{StaticCell, ConstStaticCell};
use arrayvec::ArrayVec;

use {defmt_rtt as _, panic_probe as _};

#[allow(unused)]
use cfg::{SAMPLE_RATE, SAMPLE_BITS, RESOLUTION};

mod audio_out;
mod keyboard;
// mod shift;
mod cfg;

type Mutex<T> = mutex::Mutex<CriticalSectionRawMutex, T>;

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => usb::InterruptHandler<USB>;
    I2C0_IRQ => i2c::InterruptHandler<I2C0>;
    ADC_IRQ_FIFO => adc::InterruptHandler;
    PIO0_IRQ_0 => pio::InterruptHandler<PIO0>;
});

static CORE1_STACK: ConstStaticCell<Stack<4096>> = ConstStaticCell::new(Stack::new());
static EXECUTOR0: StaticCell<Executor> = StaticCell::new();
static EXECUTOR1: StaticCell<Executor> = StaticCell::new();

static NOTES_L: AtomicU32 = AtomicU32::new(0);
static NOTES_H: AtomicU32 = AtomicU32::new(0);
static FADER_A: AtomicU8 = AtomicU8::new(0);
static FADER_B: AtomicU8 = AtomicU8::new(0);

#[cortex_m_rt::entry]
fn main() -> ! {
    let config = Config::new(cfg::clock_config());
    let p = embassy_rp::init(config);

    // LED / SERIAL INIT

    let driver = usb::Driver::new(p.USB, Irqs);
    let led = Output::new(p.PIN_25, Level::Low);

    // KEYBOARD TASK INIT

    let kb_map = keyboard::KeyboardMap {
        menu_2: Input::new(p.PIN_20, Pull::None),
        menu_14: Input::new(p.PIN_9, Pull::None),
        menu_15: Input::new(p.PIN_8, Pull::None),
        menu_16: Input::new(p.PIN_7, Pull::None),

        note_1: Input::new(p.PIN_21, Pull::None),
        note_2: Input::new(p.PIN_6, Pull::None),
        note_3: Input::new(p.PIN_10, Pull::None),
        note_4: Input::new(p.PIN_15, Pull::None),
        note_5: Input::new(p.PIN_14, Pull::None),

        adc_p1: Channel::new_pin(p.PIN_40, Pull::None),
        adc_p2: Channel::new_pin(p.PIN_41, Pull::None),
        adc: Adc::new(p.ADC, Irqs, Default::default()),
    };

    let kb_task = keyboard::kb_test(p.I2C0, p.PIN_17, p.PIN_16, kb_map);

    // AUDIO OUT TASKS

    let pio = pio::Pio::new(p.PIO0, Irqs);
    let dma_task = audio_out::dma_forward_task(pio, p.DMA_CH0, p.PIN_22);
    let gen_task = audio_out::audio_gen_task();

    // EXECUTOR INIT

    let core1_thread = move || {
        let executor1 = EXECUTOR1.init(Executor::new());
        executor1.run(|spawner| spawner.spawn(debug_task(led)).unwrap());
    };

    let core1_stack = CORE1_STACK.take();
    spawn_core1(p.CORE1, core1_stack, core1_thread);

    let executor0 = EXECUTOR0.init(Executor::new());
    executor0.run(|spawner| {
        spawner.spawn(kb_task).unwrap();
        spawner.spawn(dma_task).unwrap();
        spawner.spawn(gen_task).unwrap();
        spawner.spawn(logger_task(driver)).unwrap();
    });
}

#[embassy_executor::task]
async fn debug_task(mut led: Output<'static>) {
    sleep_ms(1000).await;

    log::info!("hello! rfreq = {}, xfreq = {}, sys = {}", rosc_freq(), xosc_freq(), clk_sys_freq());
    sleep_ms(1).await;

    loop {
        sleep_ms(10).await;
        tick_n(1, &mut led).await;
    }
}

async fn tick_n(n: usize, led: &mut Output<'_>) {
    if n > 1 {
        sleep_ms(1000).await;
    }

    for _i in 0..n {
        led.set_high();
        sleep_ms(1).await;
        led.set_low();
        sleep_ms(999).await;
    }

    if n > 1 {
        sleep_ms(1700).await;
    }
}

async fn sleep_ms(num: u64) {
    Timer::after_millis(num).await;
}

async fn sleep_us(num: u64) {
    Timer::after_micros(num).await;
}

#[embassy_executor::task]
async fn logger_task(driver: usb::Driver<'static, USB>) {
    use log::{LevelFilter, Record};
    use embassy_usb_logger::*;
    use core::fmt::Write;

    fn fmt(record: &Record<'_>, writer: &mut Writer<'_, 1024>) {
        let level = record.level().as_str();
        write!(writer, "[{level}] {}\n", record.args()).unwrap();
    }

    static LOGGER: UsbLogger::<1024, DummyHandler> = UsbLogger::with_custom_style(fmt);

    let _ = log::set_logger(&LOGGER);
    log::set_max_level(LevelFilter::Trace);

    let mut state = LoggerState::new();
    LOGGER.run(&mut state, driver).await;
}
