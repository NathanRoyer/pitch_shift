//! This example shows how to use USB (Universal Serial Bus) in the RP2040 chip.
//!
//! This creates a USB serial port that echos.

#![no_std]
#![no_main]

use log::{info};
use embassy_time::{Timer, Instant};
use embassy_executor::Executor;

use embassy_rp::clocks::{rosc_freq, xosc_freq, clk_sys_freq};
use embassy_rp::multicore::{spawn_core1, Stack};
use embassy_rp::usb::{Driver, InterruptHandler};
use embassy_rp::peripherals::USB;
use embassy_rp::bind_interrupts;
use embassy_rp::config::Config;
use embassy_rp::gpio;

use static_cell::{StaticCell, ConstStaticCell};

use gpio::{Level, Output};
use {defmt_rtt as _, panic_probe as _};

use shift::{PitchShifter, SHIFTER_INIT};

mod shift;
mod cfg;

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<USB>;
});

static SHIFTER: ConstStaticCell<PitchShifter> = ConstStaticCell::new(SHIFTER_INIT);

const INIT_BUF: [f32; 1024] = [0.0; 1024];

static IBUF: ConstStaticCell<[f32; 1024]> = ConstStaticCell::new(INIT_BUF);
static OBUF: ConstStaticCell<[f32; 1024]> = ConstStaticCell::new(INIT_BUF);

static CORE1_STACK: ConstStaticCell<Stack<4096>> = ConstStaticCell::new(Stack::new());
static EXECUTOR0: StaticCell<Executor> = StaticCell::new();
static EXECUTOR1: StaticCell<Executor> = StaticCell::new();

#[cortex_m_rt::entry]
fn main() -> ! {
    let _config = Config::new(cfg::clock_config());
    let p = embassy_rp::init(Default::default());

    let driver = Driver::new(p.USB, Irqs);
    let led = Output::new(p.PIN_25, Level::Low);

    let core1_thread = move || {
        let executor1 = EXECUTOR1.init(Executor::new());
        executor1.run(|spawner| spawner.spawn(core1_task(led)).unwrap());
    };

    let core1_stack = CORE1_STACK.take();
    spawn_core1(p.CORE1, core1_stack, core1_thread);

    let executor0 = EXECUTOR0.init(Executor::new());
    executor0.run(|spawner| {
        spawner.spawn(core0_task()).unwrap();
        spawner.spawn(logger_task(driver)).unwrap();
    });
}

#[embassy_executor::task]
async fn core1_task(mut led: Output<'static>) {
    let shifter = SHIFTER.take();
    shifter.init(48000);
    sleep_ms(1000).await;

    log::info!("hello! rfreq = {}, xfreq = {}, sys = {}", rosc_freq(), xosc_freq(), clk_sys_freq());
    sleep_ms(1).await;

    let ibuf = IBUF.take();
    let obuf = OBUF.take();

    // Do stuff with the class!
    loop {
        let then = Instant::now();

        // big compute
        shifter.shift_pitch(16, 12.0, ibuf, obuf).await;

        let elapsed = then.elapsed();
        info!("took {}ms", elapsed.as_millis());
        tick_n(1, &mut led).await;
    }
}

#[embassy_executor::task]
async fn core0_task() {
    // info!("Hello from core 0");
    loop {
        sleep_ms(1000).await;
    }
}

async fn tick_n(n: usize, led: &mut Output<'_>) {
    if n > 1 {
        sleep_ms(1000).await;
    }

    for _i in 0..n {
        led.set_high();
        sleep_ms(200).await;
        led.set_low();
        sleep_ms(800).await;
    }

    if n > 1 {
        sleep_ms(1700).await;
    }
}

async fn sleep_ms(num: u64) {
    Timer::after_millis(num).await;
}

#[embassy_executor::task]
async fn logger_task(driver: Driver<'static, USB>) {
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
