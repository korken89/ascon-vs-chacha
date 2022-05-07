#![no_main]
#![no_std]

use ascon_vs_chacha as _; // global logger + panicking-behavior + memory layout

pub mod tasks;

#[rtic::app(device = embassy_nrf::pac, dispatchers = [SWI0_EGU0])]
mod app {
    use ascon_vs_chacha::bsp;

    #[shared]
    pub struct Shared {}

    #[local]
    pub struct Local {}

    #[init]
    fn init(cx: init::Context) -> (Shared, Local) {
        defmt::info!("init");

        bsp::init(cx.core);

        task::spawn().ok();

        (Shared {}, Local {})
    }

    use crate::tasks::task;

    extern "Rust" {
        #[task]
        async fn task(cx: task::Context);
    }
}
