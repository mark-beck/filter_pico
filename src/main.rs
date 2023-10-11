#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_projections)]

use defmt::*;
use embassy_executor::Spawner;
use embassy_rp::{gpio::{self, Pin, Input}, Peripheral, Peripherals, peripherals::PIN_15, clocks, rtc::DateTime};
use embassy_time::{Duration, Timer, block_for};
use gpio::{Level, Output};
use {defmt_rtt as _, panic_probe as _};

type LED = PIN_15;

#[embassy_executor::main]
async fn main(_spawner: Spawner) {

    let p = embassy_rp::init(Default::default());
    let led1 = Output::new(p.PIN_15, Level::Low);
    let mut trig = Output::new(p.PIN_17, Level::Low);
    let mut echo = Input::new(p.PIN_16, gpio::Pull::None);

    _spawner.spawn(start_blink(led1)).expect("cant spawn blink");

    loop {
        match measure(&mut trig, &mut echo) {
            Some(d) => info!("Distance: {}", d),
            None => info!("no distance")
        }
        Timer::after(Duration::from_secs(5)).await;
    }
}

#[embassy_executor::task]
async fn start_blink(led: Output<'static, LED>) -> ! {
    blink(led).await
}

async fn blink<T: Pin>(mut led: Output<'_, T>) -> ! {
    loop {
        led.set_high();
        Timer::after(Duration::from_secs(1)).await;

        led.set_low();
        Timer::after(Duration::from_secs(1)).await;
    }
}


fn measure<T: Pin, U: Pin>(trig: &mut Output<'static, T>, echo: &mut Input<'static, U>) -> Option<u64> {

    // 10 us pulse to send wave
    trig.set_high();
    block_for(Duration::from_micros(10));
    trig.set_low();

    let time = embassy_time::Instant::now();
    while echo.is_low() {
        if time.elapsed() > Duration::from_secs(2) {
            info!("timeout waiting for high");
            return None;
        }
    }

    let time = embassy_time::Instant::now();
    while echo.is_high() {
        if time.elapsed() > Duration::from_secs(2) {
            info!("timeout waiting for low");
            return None;
        }
    }
    let past = time.elapsed();

    let distance = (past.as_ticks() * 171_605) / embassy_time::TICK_HZ;

    return Some(distance);
}