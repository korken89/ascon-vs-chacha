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
    use atomic_polyfill::{AtomicBool, Ordering};
    use core::{cell::UnsafeCell, mem::MaybeUninit, ptr};

    /// Single slot queue.
    pub struct SingleSlotQueue<T> {
        full: AtomicBool,
        val: UnsafeCell<MaybeUninit<T>>,
    }

    impl<T> SingleSlotQueue<T> {
        pub const fn new() -> Self {
            SingleSlotQueue {
                full: AtomicBool::new(false),
                val: UnsafeCell::new(MaybeUninit::uninit()),
            }
        }

        pub fn split<'a>(&'a mut self) -> (Consumer<'a, T>, Producer<'a, T>) {
            (Consumer { ssq: self }, Producer { ssq: self })
        }
    }

    impl<T> Drop for SingleSlotQueue<T> {
        fn drop(&mut self) {
            if self.full.load(Ordering::Relaxed) {
                unsafe {
                    ptr::drop_in_place(self.val.get() as *mut T);
                }
            }
        }
    }

    /// Read handle to a single slot queue.
    pub struct Consumer<'a, T> {
        ssq: &'a SingleSlotQueue<T>,
    }

    impl<'a, T> Consumer<'a, T> {
        /// Try reading a value from the queue.
        #[inline]
        pub fn dequeue(&mut self) -> Option<T> {
            if self.ssq.full.load(Ordering::Acquire) {
                let r = Some(unsafe { ptr::read(self.ssq.val.get().cast()) });
                self.ssq.full.store(false, Ordering::Release);
                r
            } else {
                None
            }
        }

        /// Check if there is a value in the queue.
        #[inline]
        pub fn is_empty(&self) -> bool {
            !self.ssq.full.load(Ordering::Relaxed)
        }
    }

    /// Safety: We gurarantee the safety using an `AtomicBool` to gate the read of the `UnsafeCell`.
    unsafe impl<'a, T> Send for Consumer<'a, T> {}

    /// Write handle to a single slot queue.
    pub struct Producer<'a, T> {
        ssq: &'a SingleSlotQueue<T>,
    }

    impl<'a, T> Producer<'a, T> {
        /// Write a value into the queue. If there is a value already in the queue this will
        /// return the value given to this method.
        #[inline]
        pub fn enqueue(&mut self, val: T) -> Option<T> {
            if !self.ssq.full.load(Ordering::Acquire) {
                unsafe { ptr::write(self.ssq.val.get().cast(), val) };
                self.ssq.full.store(true, Ordering::Release);
                None
            } else {
                Some(val)
            }
        }

        /// Check if there is a value in the queue.
        #[inline]
        pub fn is_empty(&self) -> bool {
            !self.ssq.full.load(Ordering::Relaxed)
        }
    }

    /// Safety: We gurarantee the safety using an `AtomicBool` to gate the write of the
    /// `UnsafeCell`.
    unsafe impl<'a, T> Send for Producer<'a, T> {}
}

pub mod async_spi {
    use super::ssq;
    use crate::hal::spim::{DmaTransfer, Spim};
    use core::{
        future::Future,
        pin::Pin,
        task::{Context, Poll, Waker},
    };
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
        waker_queue: ssq::SingleSlotQueue<Waker>,
    }

    impl Storage {
        /// Create a new storage.
        pub const fn new() -> Self {
            Storage {
                waker_queue: ssq::SingleSlotQueue::new(),
            }
        }

        /// Takes the a reference to static storage and a SPI and give the Async SPI handle and
        /// backend.
        pub fn split<T: Instance>(&'static mut self, spi: Spim<T>) -> (Handle<T>, Backend<T>) {
            let (r, w) = self.waker_queue.split();

            (
                Handle {
                    send_waker: w,
                    state: SpiOrTransfer::Spi(spi),
                },
                Backend {
                    waiting: r,
                    _t: core::marker::PhantomData,
                },
            )
        }
    }

    /// Handles the DMA's end interrupt and wakes up the waiting wakers.
    pub struct Backend<T: Instance> {
        waiting: ssq::Consumer<'static, Waker>,
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
            if let Some(waker) = self.waiting.dequeue() {
                defmt::trace!("    spim_interrupt: Waking a waker");
                waker.wake();
            }
        }
    }

    /// Used by drivers to access SPI, registers wakers to the DMA interrupt backend.
    pub struct Handle<T: Instance> {
        send_waker: ssq::Producer<'static, Waker>,
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
                    s.aspi.send_waker.enqueue(cx.waker().clone());

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
                    s.aspi.send_waker.enqueue(cx.waker().clone());

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
