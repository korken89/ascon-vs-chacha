use crate::hal::{
    clocks::{Clocks, LfOscConfiguration},
    gpio::{p0, Level as PinLevel, OpenDrain, OpenDrainConfig, Output, Pin, PushPull},
    gpiote::Gpiote,
    pac::{self, RTC0, SPIM0},
    spim::{self, Spim, SpimEvent},
};
use crate::rtc_monotonic::MonoRtc;

pub struct Dw1000 {
    spi: Spim<SPIM0>,
    cs: Pin<Output<PushPull>>,
    rst: Pin<Output<OpenDrain>>,
    gpiote: Gpiote,
}

pub mod async_spi {
    use crate::hal::{
        pac::SPIM0,
        spim::{DmaTransfer, Spim},
    };
    use core::{
        future::Future,
        pin::Pin,
        task::{Context, Poll, Waker},
    };
    use heapless::spsc::{Consumer, Producer, Queue};

    enum SpiOrTransfer {
        Spi(Spim<SPIM0>),
        Transfer(DmaTransfer<SPIM0, &'static mut [u8]>),
    }

    /// Storage for the queue to the async SPI
    pub struct AsyncSpiStorage {
        spi_queue: Queue<Waker, 8>,
    }

    impl AsyncSpiStorage {
        pub fn split(&mut self, spi: Spim<SPIM0>) -> (AsyncSpiHandle, AsyncSpiBackend) {
            let (p, c) = self.spi_queue.split();

            (
                AsyncSpiHandle { p },
                AsyncSpiBackend {
                    spi: SpiOrTransfer::Spi(spi),
                    waiting: c,
                },
            )
        }
    }

    /// Used by SPIM interrupt to do transfers.
    pub struct AsyncSpiBackend<'a> {
        spi: SpiOrTransfer,
        waiting: Consumer<'a, Waker, 8>,
    }

    /// Used by drivers to access SPI.
    ///
    /// TODO: How to get results from backend?
    pub struct AsyncSpiHandle<'a> {
        p: Producer<'a, Waker, 8>,
    }

    impl<'a> AsyncSpiHandle<'a> {
        fn transfer<'s>(&'s mut self, buf: &'static mut [u8]) -> AsyncSpiFuture<'s, 'a> {
            AsyncSpiFuture { buf, aspi: self }
        }
    }

    pub struct AsyncSpiFuture<'a, 'b> {
        buf: &'static mut [u8],
        aspi: &'a mut AsyncSpiHandle<'b>,
    }

    impl Future for AsyncSpiFuture<'_, '_> {
        type Output = ();

        fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
            todo!()
        }
    }
}
#[inline(always)]
pub fn init(_c: cortex_m::Peripherals, p: pac::Peripherals) -> (MonoRtc<RTC0>, Dw1000) {
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

    let spi = Spim::new(p.SPIM0, spi_pins, spim::Frequency::M2, spim::MODE_0, 0);
    spi.enable_interrupt(SpimEvent::End);

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

    (
        MonoRtc::new(p.RTC0),
        Dw1000 {
            spi,
            cs,
            rst,
            gpiote,
        },
    )
}
