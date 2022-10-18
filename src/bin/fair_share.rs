#![no_main]
#![no_std]
#![feature(type_alias_impl_trait)]
#![feature(async_fn_in_trait)]

use dwm1001_async as _; // global logger + panicking-behavior + memory layout

defmt::timestamp!("{=u64:us}", {
    let time_us: dwm1001_async::rtc_monotonic::fugit::MicrosDurationU64 =
        app::monotonics::now().duration_since_epoch().convert();

    time_us.ticks()
});

#[rtic::app(device = dwm1001_async::hal::pac, dispatchers = [SWI0_EGU0, SWI1_EGU1, SWI2_EGU2, SWI3_EGU3])]
mod app {
    use dwm1001_async::{async_spi, bsp, fair_share, hal};
    use embedded_hal_async::spi::SpiBus;
    use hal::pac::{RTC0, SPIM0};
    use hal::prelude::*;
    use systick_monotonic::*;

    #[monotonic(binds = SysTick, default = true)]
    type Mono = Systick<1000>;

    #[shared]
    struct Shared {
        fs: fair_share::FairShare<u32>,
    }

    #[local]
    struct Local {}

    #[init(local = [aspi_storage: async_spi::Storage = async_spi::Storage::new()])]
    fn init(cx: init::Context) -> (Shared, Local, init::Monotonics) {
        defmt::info!("init");

        let (mono, _, _) = bsp::init(cx.core, cx.device, cx.local.aspi_storage);

        task1::spawn().ok();
        task2::spawn().ok();
        task3::spawn().ok();
        task4::spawn().ok();

        let fs = fair_share::FairShare::new(0u32);

        (Shared { fs }, Local {}, init::Monotonics(mono))
    }

    #[task(priority = 1, shared = [&fs])]
    async fn task1(cx: task1::Context) {
        defmt::info!("    starting task 1");

        let mut access = cx.shared.fs.access().await;
        defmt::info!("    task1: got access");
        monotonics::delay(1000.millis()).await;
        *access += 1;
        defmt::info!("    task1: releasing access with val {}", *access);
        drop(access);

        monotonics::delay(5000.millis()).await;

        let mut access = cx.shared.fs.access().await;
        defmt::info!("    task1: got access");
        monotonics::delay(1000.millis()).await;
        *access += 1;
        defmt::info!("    task1: releasing access with val {}", *access);
        drop(access);

        defmt::info!("    exiting task 1");
    }

    #[task(priority = 2, shared = [&fs])]
    async fn task2(cx: task2::Context) {
        defmt::info!("        starting task 2");

        monotonics::delay(100.millis()).await;

        defmt::info!("        task2: trying to take access");
        let mut access = cx.shared.fs.access().await;
        defmt::info!("        task2: got access");
        monotonics::delay(1000.millis()).await;
        *access += 1;
        defmt::info!("        task2: releasing access with val {}", *access);
        drop(access);

        defmt::info!("        exiting task 2");
    }

    #[task(priority = 3, shared = [&fs])]
    async fn task3(cx: task3::Context) {
        defmt::info!("            starting task 3");

        monotonics::delay(200.millis()).await;

        defmt::info!("            task3: trying to take access");

        let mut access = cx.shared.fs.access().await;
        defmt::info!("            task3: got access");
        monotonics::delay(1000.millis()).await;
        *access += 1;
        defmt::info!("            task3: releasing access with val {}", *access);
        drop(access);

        defmt::info!("            exiting task 3");
    }

    #[task(priority = 4, shared = [&fs])]
    async fn task4(cx: task4::Context) {
        defmt::info!("                starting task 4");

        monotonics::delay(300.millis()).await;

        defmt::info!("                task4: trying to take access");

        let mut access = cx.shared.fs.access().await;
        defmt::info!("                task4: got access");
        monotonics::delay(1000.millis()).await;
        *access += 1;
        defmt::info!(
            "                task4: releasing access with val {}",
            *access
        );
        drop(access);

        defmt::info!("                exiting task 4");
    }
}
