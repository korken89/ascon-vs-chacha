#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]

use dwm1001_async as _; // global logger + panicking-behavior + memory layout

use core::{
    future::Future,
    mem,
    pin::Pin,
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};

defmt::timestamp!("{}", {
    let time_ms: dwm1001_async::rtc_monotonic::fugit::MillisDurationU64 =
        app::monotonics::now().duration_since_epoch().convert();

    time_ms
});

#[rtic::app(device = dwm1001_async::hal::pac, dispatchers = [SWI0_EGU0, SWI1_EGU1])]
mod app {
    use super::*;
    use dwm1001_async::{bsp, hal, rtc_monotonic::*};
    use hal::pac::{RTC0, SPIM0};
    use hal::prelude::*;

    #[monotonic(binds = RTC0, default = true)]
    type Mono = MonoRtc<RTC0>;

    pub type AppInstant = <Mono as rtic::Monotonic>::Instant;
    pub type AppDuration = <Mono as rtic::Monotonic>::Duration;

    #[shared]
    struct Shared {
        s: u32,
    }

    #[local]
    struct Local {
        async_spi_backend: async_spi::Backend<SPIM0>,
        async_spi_handle: async_spi::Handle<SPIM0>,
        dw1000: bsp::Dw1000,
    }

    use bsp::async_spi;

    #[init(local = [aspi_storage: async_spi::Storage = async_spi::Storage::new()])]
    fn init(cx: init::Context) -> (Shared, Local, init::Monotonics) {
        defmt::info!("init");

        let (mono, dw1000, aspi_handle, aspi_backend) =
            bsp::init(cx.core, cx.device, cx.local.aspi_storage);

        task1::spawn().ok();

        // task_executor::spawn().ok();
        rtic::pend(hal::pac::Interrupt::SWI2_EGU2);

        (
            Shared { s: 0 },
            Local {
                async_spi_backend: aspi_backend,
                async_spi_handle: aspi_handle,
                dw1000,
            },
            init::Monotonics(mono),
        )
    }

    #[idle]
    fn idle(_: idle::Context) -> ! {
        defmt::info!("idle");

        loop {}
    }

    #[task(binds = SPIM0_SPIS0_TWIM0_TWIS0_SPI0_TWI0, priority = 8, local = [async_spi_backend])]
    fn spim_task(cx: spim_task::Context) {
        cx.local.async_spi_backend.spim_interrupt();
    }

    #[task]
    fn task1(_cx: task1::Context) {
        let now: fugit::MinutesDurationU64 = monotonics::now().duration_since_epoch().convert();
        defmt::info!("Hello from task1! now: {}", now);

        task1::spawn_after(2.secs()).ok();
    }

    // TODO: This should be the task, that is understood by the `syntax` proc-macro
    // #[task(priority = 2)]
    async fn task(cx: task_executor::Context<'_>) {
        static mut BUF: [u8; 5] = [0; 5];
        #[allow(unused_imports)]
        use rtic::mutex_prelude::*;

        // defmt::info!("delay long time");

        loop {
            Delay::spawn(1234.millis()).await;

            defmt::info!("do SPI!!!");

            cx.local.dw1000.cs.set_low().ok();

            unsafe {
                BUF = [0; 5];
            }

            let r = unsafe { cx.local.async_spi_handle.transfer(&mut BUF).await };

            defmt::info!("SPI done! Res: {:x}", r);

            cx.local.dw1000.cs.set_high().ok();
        }

        // defmt::info!("we have just created the future");
        // fut.await;
        // defmt::info!("long delay done");

        // defmt::info!("delay short time");
        // sleep(500.millis()).await;
        // defmt::info!("short delay done");

        // defmt::info!("test timeout");
        // let res = timeout(NeverEndingFuture {}, 1.secs()).await;
        // defmt::info!("timeout done: {:?}", res);

        // defmt::info!("test timeout 2");
        // let res = timeout(Delay::spawn(500.millis()), 1.secs()).await;
        // defmt::info!("timeout done 2: {:?}", res);
    }

    //////////////////////////////////////////////
    // BEGIN BOILERPLATE
    //////////////////////////////////////////////
    type F = impl Future + 'static;
    static mut TASK: AsyncTaskExecutor<F> = AsyncTaskExecutor::new();

    // TODO: This should be a special case codegen for the `dispatcher`, which runs
    //       in the dispatcher. Not as its own task, this is just to make it work
    //       in this example.
    #[task(binds = SWI2_EGU2, shared = [s], local = [async_spi_handle, dw1000])]
    fn task_executor(cx: task_executor::Context) {
        let task_storage = unsafe { &mut TASK };
        match task_storage {
            AsyncTaskExecutor::Idle => {
                // TODO: The context generated for async tasks need 'static lifetime,
                // use `mem::transmute` for now until codegen is fixed
                //
                // TODO: Check if there is some way to not need 'static lifetime
                defmt::trace!("    task_executor spawn");
                task_storage.spawn(|| task(unsafe { mem::transmute(cx) }));
                rtic::pend(hal::pac::Interrupt::SWI2_EGU2);
            }
            _ => {
                defmt::trace!("    task_executor run");
                task_storage.poll(|| {
                    rtic::pend(hal::pac::Interrupt::SWI2_EGU2);
                });
            }
        };
    }

    // TODO: This is generated by the `delay` impl, it needs a capacity equal or grater
    //       than the number of async tasks in the system. Should more likely be a part
    //       of the monotonic codegen, not its own task.
    #[task(priority = 8, capacity = 8)]
    fn delay_handler(_: delay_handler::Context, waker: Waker) {
        waker.wake();
    }
    //////////////////////////////////////////////
    // END BOILERPLATE
    //////////////////////////////////////////////
}

//=============
// Waker

static WAKER_VTABLE: RawWakerVTable =
    RawWakerVTable::new(waker_clone, waker_wake, waker_wake, waker_drop);

unsafe fn waker_clone(p: *const ()) -> RawWaker {
    RawWaker::new(p, &WAKER_VTABLE)
}

unsafe fn waker_wake(p: *const ()) {
    // The only thing we need from a waker is the function to call to pend the async
    // dispatcher.
    let f: fn() = mem::transmute(p);
    f();
}

unsafe fn waker_drop(_: *const ()) {
    // nop
}

//============
// AsyncTaskExecutor

enum AsyncTaskExecutor<F: Future + 'static> {
    Idle,
    Running(F),
}

impl<F: Future + 'static> AsyncTaskExecutor<F> {
    const fn new() -> Self {
        Self::Idle
    }

    fn spawn(&mut self, future: impl FnOnce() -> F) {
        *self = AsyncTaskExecutor::Running(future());
    }

    fn poll(&mut self, wake: fn()) {
        match self {
            AsyncTaskExecutor::Idle => {}
            AsyncTaskExecutor::Running(future) => unsafe {
                let waker_data: *const () = mem::transmute(wake);
                let waker = Waker::from_raw(RawWaker::new(waker_data, &WAKER_VTABLE));
                let mut cx = Context::from_waker(&waker);
                let future = Pin::new_unchecked(future);

                match future.poll(&mut cx) {
                    Poll::Ready(_) => {
                        *self = AsyncTaskExecutor::Idle;
                        defmt::trace!("    task_executor idle");
                    }
                    Poll::Pending => {}
                };
            },
        }
    }
}

//=============
// Delay

pub struct Delay {
    until: crate::app::AppInstant,
}

impl Delay {
    pub fn spawn(duration: crate::app::AppDuration) -> Self {
        let until = crate::app::monotonics::now() + duration;

        Delay { until }
    }
}

#[inline(always)]
pub fn sleep(duration: crate::app::AppDuration) -> Delay {
    Delay::spawn(duration)
}

impl Future for Delay {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let s = self.as_mut();
        let now = crate::app::monotonics::now();

        defmt::trace!("    poll Delay");

        if now >= s.until {
            Poll::Ready(())
        } else {
            let waker = cx.waker().clone();
            crate::app::delay_handler::spawn_after(s.until - now, waker).ok();

            Poll::Pending
        }
    }
}

//=============
// Timeout future

#[derive(Copy, Clone, Debug, defmt::Format)]
pub struct TimeoutError;

pub struct Timeout<F: Future> {
    future: F,
    until: crate::app::AppInstant,
    cancel_handle: Option<crate::app::delay_handler::SpawnHandle>,
}

impl<F> Timeout<F>
where
    F: Future,
{
    pub fn timeout(future: F, duration: crate::app::AppDuration) -> Self {
        let until = crate::app::monotonics::now() + duration;
        Self {
            future,
            until,
            cancel_handle: None,
        }
    }
}

#[inline(always)]
pub fn timeout<F: Future>(future: F, duration: crate::app::AppDuration) -> Timeout<F> {
    Timeout::timeout(future, duration)
}

impl<F> Future for Timeout<F>
where
    F: Future,
{
    type Output = Result<F::Output, TimeoutError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let now = crate::app::monotonics::now();

        // SAFETY: We don't move the underlying pinned value.
        let mut s = unsafe { self.get_unchecked_mut() };
        let future = unsafe { Pin::new_unchecked(&mut s.future) };

        defmt::trace!("    poll Timeout");

        match future.poll(cx) {
            Poll::Ready(r) => {
                if let Some(ch) = s.cancel_handle.take() {
                    ch.cancel().ok();
                }

                Poll::Ready(Ok(r))
            }
            Poll::Pending => {
                if now >= s.until {
                    Poll::Ready(Err(TimeoutError))
                } else if s.cancel_handle.is_none() {
                    let waker = cx.waker().clone();
                    let sh = crate::app::delay_handler::spawn_after(s.until - now, waker)
                        .expect("Internal RTIC bug, this should never fail");
                    s.cancel_handle = Some(sh);

                    Poll::Pending
                } else {
                    Poll::Pending
                }
            }
        }
    }
}

pub struct NeverEndingFuture {}

impl Future for NeverEndingFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        // Never finish
        defmt::trace!("    polling NeverEndingFuture");
        Poll::Pending
    }
}

//=============
// Async SPI driver
