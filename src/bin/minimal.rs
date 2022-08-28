#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(generic_associated_types)]

use dwm1001_async as _; // global logger + panicking-behavior + memory layout

defmt::timestamp!("{=u64:us}", {
    let time_us: dwm1001_async::rtc_monotonic::fugit::MicrosDurationU64 =
        app::monotonics::now().duration_since_epoch().convert();

    time_us.ticks()
});

#[rtic::app(device = dwm1001_async::hal::pac, dispatchers = [SWI0_EGU0])]
mod app {
    use dwm1001_async::{async_spi, bsp, hal, rtc_monotonic::*};
    use embedded_hal_async::spi::SpiBus;
    use hal::pac::{RTC0, SPIM0};
    use hal::prelude::*;

    #[monotonic(binds = RTC0, default = true)]
    type Mono = MonoRtc<RTC0>;

    #[shared]
    struct Shared {}

    #[local]
    struct Local {
        async_spi_handle: async_spi::Handle<SPIM0>,
        dw1000: bsp::Dw1000,
    }

    #[init(local = [aspi_storage: async_spi::Storage = async_spi::Storage::new()])]
    fn init(cx: init::Context) -> (Shared, Local, init::Monotonics) {
        defmt::info!("init");

        let (mono, dw1000, aspi_handle) = bsp::init(cx.core, cx.device, cx.local.aspi_storage);

        async_task::spawn().ok();

        (
            Shared {},
            Local {
                async_spi_handle: aspi_handle,
                dw1000,
            },
            init::Monotonics(mono),
        )
    }

    #[task(local = [async_spi_handle, dw1000])]
    async fn async_task(cx: async_task::Context) {
        let spi = cx.local.async_spi_handle;
        let cs = &mut cx.local.dw1000.cs;

        // defmt::info!("delay long time");

        loop {
            monotonics::delay(100.millis()).await;

            cs.set_low().ok();

            let mut buf = [0; 5];

            spi.transfer_in_place(&mut buf).await.ok();

            defmt::info!("SPI done! Res: {:x}", buf);

            cs.set_high().ok();
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
}
