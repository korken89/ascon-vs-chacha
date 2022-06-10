use crate::{
    hal::spim::{DmaTransfer, Spim},
    ssq,
};
use core::{
    future::Future,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll, Waker},
};
use nrf52832_hal::spim::{Instance, SpimEvent};

pub type WakerQueue = ssq::SingleSlotQueue<Waker>;
pub type WakerProducer<'a> = ssq::Producer<'a, Waker>;
pub type WakerConsumer<'a> = ssq::Consumer<'a, Waker>;

/// Aync SPI state.
enum SpiOrTransfer<T: Instance> {
    /// Used when moving between the two states.
    Intermediate,
    /// SPI idle.
    Spi(Spim<T>),
    /// SPI in active DMA transfer.
    Transfer(DmaTransfer<T, DmaSlice>),
}

// impl<T: Instance> Drop for SpiOrTransfer<T> {
//     fn drop(&mut self) {
//         match self {
//             SpiOrTransfer::Transfer(_transfer) => {
//                 panic!("ops, this is not implemented");
//                 // The HAL does not support aborting a transfer yet, need adding
//                 // transfer.abort();
//             }
//             _ => {}
//         }
//     }
// }

unsafe impl<T: Instance> Send for SpiOrTransfer<T> {}

/// Storage for the queue to the async SPI's wakers, place this in 'static storage.
pub struct Storage {
    waker_queue: WakerQueue,
}

impl Storage {
    /// Create a new storage.
    pub const fn new() -> Self {
        Storage {
            waker_queue: WakerQueue::new(),
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
                _t: PhantomData,
            },
        )
    }
}

/// Handles the DMA's end interrupt and wakes up the waiting wakers.
pub struct Backend<T: Instance> {
    waiting: WakerConsumer<'static>,
    _t: PhantomData<T>,
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
    send_waker: WakerProducer<'static>,
    state: SpiOrTransfer<T>,
}

impl<T: Instance> Handle<T> {
    /// Perform an SPI transfer.
    pub fn transfer<'s>(&'s mut self, buf: &'s mut [u8]) -> TransferFuture<'s, T> {
        defmt::trace!("    Handle: Creating TransferFuture...");
        TransferFuture {
            buf: unsafe { DmaSlice::from_slice(buf) },
            aspi: self,
        }
    }
}

struct DmaSlice {
    ptr: *mut u8,
    len: usize,
}

impl Default for DmaSlice {
    fn default() -> Self {
        Self {
            ptr: core::ptr::null_mut(),
            len: 0,
        }
    }
}

impl DmaSlice {
    /// Take a slice and generate an DmaSlice for `async` usage.
    ///
    /// Safety: This is only safe to use within an async future.
    pub unsafe fn from_slice(buf: &mut [u8]) -> Self {
        Self {
            ptr: buf.as_mut_ptr(),
            len: buf.len(),
        }
    }

    /// Take a DmaSlice for `async` usage and give back the underlying buffer
    pub fn to_slice<'a>(self) -> &'a mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}

unsafe impl embedded_dma::WriteBuffer for DmaSlice {
    type Word = u8;

    unsafe fn write_buffer(&mut self) -> (*mut Self::Word, usize) {
        (self.ptr, self.len)
    }
}

/// Handles the `async` part of the SPI DMA transfer
pub struct TransferFuture<'a, T: Instance> {
    buf: DmaSlice,
    aspi: &'a mut Handle<T>,
}

impl<'a, T: Instance> Future for TransferFuture<'a, T> {
    type Output = &'a mut [u8];

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
                let transfer = spi.dma_transfer(core::mem::take(&mut s.buf));
                s.aspi.state = SpiOrTransfer::Transfer(transfer);
                defmt::trace!("    TransferFuture: Starting transfer...");
            }
            SpiOrTransfer::Transfer(transfer) => {
                if transfer.is_done() {
                    // Get the SPI and buffer back
                    let (buf, spi) = transfer.wait();
                    s.aspi.state = SpiOrTransfer::Spi(spi);

                    defmt::trace!("    TransferFuture: Transfer done!");

                    return Poll::Ready(buf.to_slice::<'a>());
                }

                defmt::trace!("    TransferFuture: Transfer not done...");

                // Enqueue a waker and enable the interrupt again so we get run again on the
                // next event
                defmt::trace!("    TransferFuture: Enqueueing waker...");
                s.aspi.send_waker.enqueue(cx.waker().clone());
                unsafe { &*T::ptr() }.intenset.write(|w| w.end().set_bit());

                // Transfer not done, place it back into the state
                s.aspi.state = SpiOrTransfer::Transfer(transfer);
            }
            SpiOrTransfer::Intermediate => unreachable!(),
        }

        Poll::Pending
    }
}
