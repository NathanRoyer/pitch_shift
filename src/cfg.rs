use embassy_rp::clocks::*;

pub fn clock_config() -> ClockConfig {
    let mut cfg = ClockConfig::rosc();

    // 180_633_600 = 180 MHz
    let main_clk = 44100 * 2048 * 2;

    cfg.rosc = Some(RoscConfig {
        hz: main_clk,
        range: RoscRange::High,
        drive_strength: [0; 8],
        div: 1,
    });

    cfg.xosc = Some(XoscConfig {
        hz: 12_000_000,
        sys_pll: None,
        usb_pll: Some(PllConfig {
            refdiv: 1,
            fbdiv: 120,
            post_div1: 6,
            post_div2: 5,
        }),
        delay_multiplier: 128,
    });

    cfg.ref_clk = RefClkConfig {
        src: RefClkSrc::Rosc,
        div: 1,
    };

    cfg.sys_clk = SysClkConfig {
        src: SysClkSrc::Rosc,
        div_int: 1,
        div_frac: 0,
    };

    cfg.peri_clk_src = Some(PeriClkSrc::Rosc);

    cfg.usb_clk = Some(UsbClkConfig {
        src: UsbClkSrc::PllUsb,
        div: 1,
        phase: 0,
    });

    cfg.adc_clk = Some(AdcClkConfig {
        src: AdcClkSrc::PllUsb,
        div: 1,
        phase: 0,
    });

    cfg
}
