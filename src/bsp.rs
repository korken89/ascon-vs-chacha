use crate::hal::{
    clocks::{Clocks, LfOscConfiguration},
    gpio::{p0, Level as PinLevel, OpenDrain, OpenDrainConfig, Output, Pin, PushPull},
    gpiote::Gpiote,
    pac::{self, RTC0, SPIM0},
    spim::{self, Spim},
};
use crate::rtc_monotonic::MonoRtc;

pub struct Dw1000 {
    pub cs: Pin<Output<PushPull>>,
    pub rst: Pin<Output<OpenDrain>>,
    pub gpiote: Gpiote,
}

pub mod ssq {
    use core::{cell::UnsafeCell, task::Waker};

    pub struct Ssq {
        val: UnsafeCell<Option<Waker>>,
    }

    impl Ssq {
        pub const fn new() -> Self {
            Ssq {
                val: UnsafeCell::new(None),
            }
        }

        pub fn split<'a>(&'a mut self) -> (Read<'a>, Write<'a>) {
            (Read { val: &self.val }, Write { val: &self.val })
        }
    }

    pub struct Read<'a> {
        val: &'a UnsafeCell<Option<Waker>>,
    }

    impl<'a> Read<'a> {
        // ...
    }

    pub struct Write<'a> {
        val: &'a UnsafeCell<Option<Waker>>,
    }

    impl<'a> Write<'a> {
        // ...
    }
}

pub mod async_spi {
    use crate::hal::spim::{DmaTransfer, Spim};
    use core::{
        future::Future,
        pin::Pin,
        task::{Context, Poll, Waker},
    };
    use heapless::spsc::{Consumer, Producer, Queue};
    use nrf52832_hal::spim::{Instance, SpimEvent};

    /// Aync SPI state.
    enum SpiOrTransfer<T: Instance> {
        /// Used when moving between the two states.
        Intermediate,
        /// SPI idle.
        Spi(Spim<T>),
        /// SPI in active DMA transfer.
        Transfer(DmaTransfer<T, &'static mut [u8]>),
    }

    /// Storage for the queue to the async SPI's wakers, place this in 'static storage.
    pub struct Storage {
        waker_queue: Queue<Waker, 2>,
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
        pub fn split<T: Instance>(&'static mut self, spi: Spim<T>) -> (Handle<T>, Backend<T>) {
            let (p, c) = self.waker_queue.split();

            (
                Handle {
                    send_waker: p,
                    state: SpiOrTransfer::Spi(spi),
                },
                Backend {
                    waiting: c,
                    _t: core::marker::PhantomData,
                },
            )
        }
    }

    /// Handles the DMA's end interrupt and wakes up the waiting wakers.
    pub struct Backend<T: Instance> {
        waiting: Consumer<'static, Waker, 2>,
        _t: core::marker::PhantomData<T>,
    }

    impl<T: Instance> Backend<T> {
        /// Run this in the SPIM interrupt.
        pub fn spim_interrupt(&mut self) {
            // Disable interrupt (clearing of the flag is done by the async polling).
            // Should probably check for other events as well. TODO: Some day.
            // TODO: Figure out a way to do this nicer
            let spi = unsafe { &*T::ptr() };
            spi.intenclr.write(|w| w.end().set_bit());

            defmt::trace!("    spim_interrupt");

            // Wake all wakers on interrupt.
            // TODO: Should do something smarter
            while let Some(waker) = self.waiting.dequeue() {
                defmt::trace!("    spim_interrupt: Waking a waker");
                waker.wake();
            }
        }
    }

    /// Used by drivers to access SPI, registers wakers to the DMA interrupt backend.
    pub struct Handle<T: Instance> {
        send_waker: Producer<'static, Waker, 2>,
        state: SpiOrTransfer<T>,
    }

    impl<T: Instance> Handle<T> {
        /// Perform an SPI transfer.
        pub fn transfer<'s>(&'s mut self, buf: &'static mut [u8]) -> TransferFuture<'s, T> {
            defmt::trace!("    Handle: Creating TransferFuture...");
            TransferFuture {
                buf: Some(buf),
                aspi: self,
            }
        }
    }

    /// Handles the `async` part of the SPI DMA transfer
    pub struct TransferFuture<'a, T: Instance> {
        buf: Option<&'static mut [u8]>,
        aspi: &'a mut Handle<T>,
    }

    impl<T: Instance> Future for TransferFuture<'_, T> {
        type Output = &'static mut [u8];

        fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
            let s = unsafe { self.get_unchecked_mut() };

            defmt::trace!("    TransferFuture: Polling...");

            let state = core::mem::replace(&mut s.aspi.state, SpiOrTransfer::Intermediate);

            match state {
                SpiOrTransfer::Spi(spi) => {
                    // The async SPI works on using the end interrupt
                    spi.reset_event(SpimEvent::End);
                    spi.enable_interrupt(SpimEvent::End);

                    // Enqueue a waker so we get run again on the next event
                    defmt::trace!("    TransferFuture: Enqueueing waker...");
                    s.aspi.send_waker.enqueue(cx.waker().clone()).ok();

                    // Start transfer.
                    let transfer = spi.dma_transfer(s.buf.take().unwrap_or_else(|| unreachable!()));
                    s.aspi.state = SpiOrTransfer::Transfer(transfer);
                    defmt::trace!("    TransferFuture: Starting transfer...");
                }
                SpiOrTransfer::Transfer(transfer) => {
                    if transfer.is_done() {
                        // Get the SPI and buffer back
                        let (buf, spi) = transfer.wait();
                        s.aspi.state = SpiOrTransfer::Spi(spi);

                        defmt::trace!("    TransferFuture: Transfer done!");

                        return Poll::Ready(buf);
                    }

                    defmt::trace!("    TransferFuture: Transfer not done...");

                    // Enqueue a waker so we get run again on the next event
                    defmt::trace!("    TransferFuture: Enqueueing waker...");
                    s.aspi.send_waker.enqueue(cx.waker().clone()).ok();

                    // Transfer not done, place it back into the state
                    s.aspi.state = SpiOrTransfer::Transfer(transfer);
                }
                SpiOrTransfer::Intermediate => unreachable!(),
            }

            Poll::Pending
        }
    }
}

#[inline(always)]
pub fn init(
    _c: cortex_m::Peripherals,
    p: pac::Peripherals,
    aspi_storage: &'static mut async_spi::Storage,
) -> (
    MonoRtc<RTC0>,
    Dw1000,
    async_spi::Handle<SPIM0>,
    async_spi::Backend<SPIM0>,
) {
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
