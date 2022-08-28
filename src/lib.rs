#![no_std]
#![feature(generic_associated_types)]

use defmt_rtt as _; // global logger
pub use nrf52832_hal as hal; // memory layout

use panic_probe as _;
// use panic_reset as _;

pub mod async_spi;
pub mod bsp;
pub mod rtc_monotonic;
pub mod ssq;
pub mod timer_monotonic;

// same panicking *behavior* as `panic-probe` but doesn't print a panic message
// this prevents the panic message being printed *twice* when `defmt::panic` is invoked
#[defmt::panic_handler]
fn panic() -> ! {
    cortex_m::asm::udf()
}

/// Terminates the application and makes `probe-run` exit with exit-code = 0
pub fn exit() -> ! {
    loop {
        cortex_m::asm::bkpt();
    }
}
