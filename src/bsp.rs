use crate::hal::{
    clocks::{Clocks, LfOscConfiguration},
    gpio::{p0, Level as PinLevel, OpenDrain, OpenDrainConfig, Output, Pin, PushPull},
    gpiote::Gpiote,
    pac::{self, RTC0, SPIM0},
    spim::{self, Spim, SpimEvent},
};
use crate::rtc_monotonic::MonoRtc;

pub struct Dw1000 {
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
    use nrf52832_hal::spim::SpimEvent;

    /// Aync SPI state.
    enum SpiOrTransfer {
        /// Used when moving between the two states.
        Intermediate,
        /// SPI idle.
        Spi(Spim<SPIM0>),
        /// SPI in active DMA transfer.
        Transfer(DmaTransfer<SPIM0, &'static mut [u8]>),
    }

    /// Storage for the queue to the async SPI's wakers, place this in 'static storage.
    pub struct Storage {
        waker_queue: Queue<Waker, 8>,
    }

    impl Storage {
        /// Create a new storage.
        pub const fn new() -> Self {
            Storage {
                waker_queue: Queue::new(),
            }
        }

        /// Takes the a reference to static storage and a SPI and give the Async SPI handle and
        /// backend.
        pub fn split(&'static mut self, spi: Spim<SPIM0>) -> (Handle, Backend) {
            let (p, c) = self.waker_queue.split();

            // The async SPI works on using the end interrupt
            spi.enable_interrupt(SpimEvent::End);

            (
                Handle {
                    send_waker: p,
                    state: SpiOrTransfer::Spi(spi),
                },
                Backend { waiting: c },
            )
        }
    }

    /// Handles the DMA's end interrupt and wakes up the waiting wakers.
    pub struct Backend {
        waiting: Consumer<'static, Waker, 8>,
    }

    impl Backend {
        /// Run this in the SPIM interrupt.
        pub fn spim_interrupt(&mut self) {
            // Clear interrupt, should probably check for other events as well. TODO: Some day.
            // TODO: Figure out a way to do this nicer
            let spi = unsafe { &*SPIM0::ptr() };
            spi.events_end.reset();

            // Wake all wakers on interrupt.
            // TODO: Should do something smarter
            while let Some(waker) = self.waiting.dequeue() {
                waker.wake();
            }
        }
    }

    /// Used by drivers to access SPI, registers wakers to the DMA interrupt backend.
    pub struct Handle {
        send_waker: Producer<'static, Waker, 8>,
        state: SpiOrTransfer,
    }

    impl Handle {
        /// Perform an SPI transfer.
        pub fn transfer<'s>(&'s mut self, buf: &'static mut [u8]) -> TransferFuture<'s> {
            TransferFuture {
                buf: Some(buf),
                aspi: self,
            }
        }
    }

    /// Handles the `async` part of the SPI DMA transfer
    pub struct TransferFuture<'a> {
        buf: Option<&'static mut [u8]>,
        aspi: &'a mut Handle,
    }

    impl Future for TransferFuture<'_> {
        type Output = &'static mut [u8];

        fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
            let s = unsafe { self.get_unchecked_mut() };

            let state = core::mem::replace(&mut s.aspi.state, SpiOrTransfer::Intermediate);

            match state {
                SpiOrTransfer::Spi(spi) => {
                    // Start transfer.
                    let transfer = spi.dma_transfer(s.buf.take().unwrap_or_else(|| unreachable!()));
                    s.aspi.state = SpiOrTransfer::Transfer(transfer);
                }
                SpiOrTransfer::Transfer(transfer) => {
                    if transfer.is_done() {
                        // Get the SPI and buffer back
                        let (buf, spi) = transfer.wait();
                        s.aspi.state = SpiOrTransfer::Spi(spi);

                        return Poll::Ready(buf);
                    }

                    // Transfer not done, place it back into the state
                    s.aspi.state = SpiOrTransfer::Transfer(transfer);
                }
                SpiOrTransfer::Intermediate => unreachable!(),
            }

            // Enqueue a waker so we get run again on the next event
            s.aspi.send_waker.enqueue(cx.waker().clone()).ok();

            Poll::Pending
        }
    }
}

#[inline(always)]
pub fn init(
    _c: cortex_m::Peripherals,
    p: pac::Peripherals,
    aspi_storage: &'static mut async_spi::Storage,
) -> (MonoRtc<RTC0>, Dw1000, async_spi::Handle, async_spi::Backend) {
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
    let (handle, backend) = aspi_storage.split(spi);

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
        Dw1000 { cs, rst, gpiote },
        handle,
        backend,
    )
}
