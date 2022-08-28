use crate::{
    async_spi,
    hal::{
        clocks::{Clocks, LfOscConfiguration},
        gpio::{p0, Level as PinLevel, OpenDrain, OpenDrainConfig, Output, Pin, PushPull},
        gpiote::Gpiote,
        pac::{self, RTC0, SPIM0},
        spim::{self, Spim},
    },
    rtc_monotonic::MonoRtc,
};

pub struct Dw1000 {
    pub cs: Pin<Output<PushPull>>,
    pub rst: Pin<Output<OpenDrain>>,
    pub gpiote: Gpiote,
}

#[inline(always)]
pub fn init(
    _c: cortex_m::Peripherals,
    p: pac::Peripherals,
    aspi_storage: &'static mut async_spi::Storage,
) -> (MonoRtc<RTC0>, Dw1000, async_spi::Handle<SPIM0>) {
    let _clocks = Clocks::new(p.CLOCK)
        .enable_ext_hfosc()
        .set_lfclk_src_external(LfOscConfiguration::NoExternalNoBypass)
        .start_lfclk();

    let port0 = p0::Parts::new(p.P0);

    let (spi_pins, cs, irq, rst, btn) = {
        let spiclk = port0.p0_16.into_push_pull_output(PinLevel::Low).degrade();
        let spimosi = port0.p0_20.into_push_pull_output(PinLevel::Low).degrade();
        let spimiso = port0.p0_18.into_floating_input().degrade();
        let cs = port0.p0_17.into_push_pull_output(PinLevel::High).degrade();
        let irq = port0.p0_19.into_floating_input().degrade();
        let rst = port0
            .p0_24
            .into_open_drain_output(OpenDrainConfig::Standard0Disconnect1, PinLevel::High)
            .degrade();

        // Not used, set to a safe value and drop
        let _wakeup = port0.p0_28.into_push_pull_output(PinLevel::Low);

        let btn = port0.p0_27.into_pullup_input().degrade();

        (
            spim::Pins {
                sck: spiclk,
                miso: Some(spimiso),
                mosi: Some(spimosi),
            },
            cs,
            irq,
            rst,
            btn,
        )
    };

    let mut spi = Spim::new(p.SPIM0, spi_pins, spim::Frequency::M1, spim::MODE_0, 0);

    // Read DEV_ID (cmd = 0x00, 4 byte length)
    let mut buf = [0; 5];
    let mut cs = cs;

    spi.transfer(&mut cs, &mut buf).unwrap();

    defmt::info!("DEV_ID: {:x}", buf);

    let handle = aspi_storage.split(spi);

    let gpiote = Gpiote::new(p.GPIOTE);

    // DW1000 IRQ
    gpiote
        .channel0()
        .input_pin(&irq)
        .lo_to_hi()
        .enable_interrupt();

    // Button IRQ
    gpiote
        .channel1()
        .input_pin(&btn)
        .hi_to_lo()
        .enable_interrupt();

    (MonoRtc::new(p.RTC0), Dw1000 { cs, rst, gpiote }, handle)
}
