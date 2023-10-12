#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]
#![feature(impl_trait_projections)]

mod valve;
mod state;

use cyw43::{Control, NetDriver};
use cyw43_pio::PioSpi;
use defmt::*;
use embassy_executor::Spawner;
use embassy_net::{Stack, Config, StackResources};
use embassy_rp::{gpio::{self, Pin, Input}, peripherals::{PIN_15, PIN_23, PIN_25, PIO0, DMA_CH0, PIN_17, PIN_16, PIN_18}, pio::{Pio, InterruptHandler}, bind_interrupts};
use embassy_time::{Duration, Timer, block_for};
use gpio::{Level, Output};
use static_cell::make_static;
use {defmt_rtt as _, panic_probe as _};
use embassy_sync::{blocking_mutex, mutex::Mutex};

type LED = PIN_18;

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => InterruptHandler<PIO0>;
});

const WIFI_NETWORK: &str = "Pixel_9770";
const WIFI_PASSWORD: &str = "12345678";

const WATERLEVEL_FILL_START: u64 = 500;
const WATERLEVEL_FILL_END: u64 = 50;

static STATE: Mutex<blocking_mutex::raw::CriticalSectionRawMutex, state::Context> = Mutex::new(state::Context{
    state: state::State{
        filter_state: state::FilterState::Idle,
        last_state_change: 0,
        waterlevel: None,
        leak: false,
    },
    config: state::Config{
        waterlevel_fill_start: WATERLEVEL_FILL_START,
        waterlevel_fill_end: WATERLEVEL_FILL_END,
        clean_before_fill_duration: 10 * embassy_time::TICK_HZ,
        clean_after_fill_duration: 10 * embassy_time::TICK_HZ,
        leak_protection: true,
    }
});

#[embassy_executor::task]
async fn wifi_task(
    runner: cyw43::Runner<'static, Output<'static, PIN_23>, PioSpi<'static, PIN_25, PIO0, 0, DMA_CH0>>,
) -> ! {
    runner.run().await
}

#[embassy_executor::task]
async fn net_task(stack: &'static Stack<cyw43::NetDriver<'static>>) -> ! {
    stack.run().await
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {

    let p = embassy_rp::init(Default::default());

    let fw = include_bytes!("../../../embassy/cyw43-firmware/43439A0.bin");
    let clm = include_bytes!("../../../embassy/cyw43-firmware/43439A0_clm.bin");

    let pwr = Output::new(p.PIN_23, Level::Low);
    let cs = Output::new(p.PIN_25, Level::High);
    let mut pio = Pio::new(p.PIO0, Irqs);
    let spi = PioSpi::new(&mut pio.common, pio.sm0, pio.irq0, cs, p.PIN_24, p.PIN_29, p.DMA_CH0);

    let state = make_static!(cyw43::State::new());
    let (net_device, mut control, runner) = cyw43::new(state, pwr, spi, fw).await;
    unwrap!(spawner.spawn(wifi_task(runner)));

    control.init(clm).await;
    control
        .set_power_management(cyw43::PowerManagementMode::PowerSave)
        .await;

    let config = Config::dhcpv4(Default::default());
    //let config = embassy_net::Config::ipv4_static(embassy_net::StaticConfigV4 {
    //    address: Ipv4Cidr::new(Ipv4Address::new(192, 168, 69, 2), 24),
    //    dns_servers: Vec::new(),
    //    gateway: Some(Ipv4Address::new(192, 168, 69, 1)),
    //});

    // Generate random seed
    let seed = 0x0123_4567_89ab_cdef; // chosen by fair dice roll. guarenteed to be random.

    // Init network stack
    let stack = &*make_static!(Stack::new(
        net_device,
        config,
        make_static!(StackResources::<2>::new()),
        seed
    ));

    unwrap!(spawner.spawn(net_task(stack)));
    spawner.spawn(start_network(control, stack)).unwrap();

    // init led pin
    let led1 = Output::new(p.PIN_18, Level::Low);

    // init ultrasonic sensor pins
    let trig = Output::new(p.PIN_17, Level::Low);
    let echo = Input::new(p.PIN_16, gpio::Pull::None);

    // init Valve controller
    let valve1 = valve::Valve::new(Output::new(p.PIN_12, Level::Low));
    let valve2 = valve::Valve::new(Output::new(p.PIN_13, Level::Low));
    let valve3 = valve::Valve::new(Output::new(p.PIN_14, Level::Low));
    let valve4 = valve::Valve::new(Output::new(p.PIN_15, Level::Low));
    let valve_controler = valve::ValveControler::new(valve1, valve2, valve3, valve4);

    spawner.spawn(blink_and_update_task(led1)).expect("cant spawn blink task");
    spawner.spawn(state_update_task(valve_controler)).expect("cant spawn state update task");
    spawner.spawn(measure_task(trig, echo)).expect("cant spawn measure task");

    loop {
        Timer::after(Duration::from_secs(5)).await;
    }
}

#[embassy_executor::task]
async fn start_network(mut control: Control<'static>, stack: &'static Stack<NetDriver<'static>>) -> ! {
    loop {
        //control.join_open(WIFI_NETWORK).await;
        match control.join_wpa2(WIFI_NETWORK, WIFI_PASSWORD).await {
            Ok(_) => break,
            Err(err) => {
                info!("join failed with status={}", err.status);
            }
        }
    }

    // Wait for DHCP, not necessary when using static IP
    info!("waiting for DHCP...");
    while !stack.is_config_up() {
        Timer::after(Duration::from_millis(100)).await;
    }
    let local_addr = stack.config_v4().unwrap().address.address();
    info!("IP address: {:?}", local_addr);

    // And now we can use it!

    let mut rx_buffer = [0; 4096];
    let mut tx_buffer = [0; 4096];
    let mut _buf = [0; 4096];

    loop {
        let _socket = embassy_net::tcp::TcpSocket::new(&stack, &mut rx_buffer, &mut tx_buffer);
        loop {
            Timer::after(Duration::from_secs(1)).await;
        }
    }
}

#[embassy_executor::task]
async fn blink_and_update_task(mut led: Output<'static, LED>) -> ! {
    loop {
        blink(&mut led);
        update_serial().await;
        Timer::after(Duration::from_secs(1)).await;
    }
}

fn blink<T: Pin>(led: &mut Output<'_, T>) {
    led.toggle();
}

async fn update_serial() {
    let c = STATE.lock().await;
    info!("State: {}", c.state);
}



#[embassy_executor::task]
async fn state_update_task(mut valve_controler: valve::ValveControler) -> ! {
    loop {
        update_state(&mut valve_controler).await;
        Timer::after(Duration::from_secs(1)).await;
    }
}


async fn update_state(valve_controler: &mut valve::ValveControler) {

        let mut c = STATE.lock().await;

        // Check for leak if enabled
        if c.config.leak_protection && c.state.leak {
            valve_controler.idle();
            if c.state.filter_state != state::FilterState::Idle {
                c.state.filter_state = state::FilterState::Idle;
                c.state.last_state_change = embassy_time::Instant::now().as_ticks();
            }
            return;
        }

        // Check if waterlevel is known
        if c.state.waterlevel.is_none() {
            return;
        }

        // Update state
        match c.state.filter_state {
            state::FilterState::CleanBeforeFill => {
                // check if we are done cleaning
                if c.state.last_state_change + c.config.clean_before_fill_duration < embassy_time::Instant::now().as_ticks() {
                    c.state.filter_state = state::FilterState::Fill;
                    c.state.last_state_change = embassy_time::Instant::now().as_ticks();
                }
            },
            state::FilterState::CleanAfterFill => {
                // check if we are done cleaning
                if c.state.last_state_change + c.config.clean_after_fill_duration < embassy_time::Instant::now().as_ticks() {
                    c.state.filter_state = state::FilterState::Idle;
                    c.state.last_state_change = embassy_time::Instant::now().as_ticks();
                }
            },
            state::FilterState::Fill => {
                // check if we are done filling
                if c.state.waterlevel.unwrap() < c.config.waterlevel_fill_end {
                    c.state.filter_state = state::FilterState::CleanAfterFill;
                    c.state.last_state_change = embassy_time::Instant::now().as_ticks();
                }   
            },
            state::FilterState::Idle => {
                // check if we need to fill
                if c.state.waterlevel.unwrap() > c.config.waterlevel_fill_start {
                    c.state.filter_state = state::FilterState::CleanBeforeFill;
                    c.state.last_state_change = embassy_time::Instant::now().as_ticks();
                }
            },
            state::FilterState::ForcedFill(time) => {
                // check if we are done filling
                if c.state.last_state_change + time < embassy_time::Instant::now().as_ticks() {
                    c.state.filter_state = state::FilterState::Idle;
                    c.state.last_state_change = embassy_time::Instant::now().as_ticks();
                }
            },
            state::FilterState::ForcedClean(time) => {
                // check if we are done cleaning
                if c.state.last_state_change + time < embassy_time::Instant::now().as_ticks() {
                    c.state.filter_state = state::FilterState::Idle;
                    c.state.last_state_change = embassy_time::Instant::now().as_ticks();
                }
            },
            state::FilterState::ForcedIdle(time) => {
                // check if we are done idling
                if c.state.last_state_change + time < embassy_time::Instant::now().as_ticks() {
                    c.state.filter_state = state::FilterState::Idle;
                    c.state.last_state_change = embassy_time::Instant::now().as_ticks();
                }
            },
        }

        // Update valve state
        match c.state.filter_state {
            state::FilterState::CleanBeforeFill => valve_controler.clean(),
            state::FilterState::CleanAfterFill => valve_controler.clean(),
            state::FilterState::Fill => valve_controler.fill(),
            state::FilterState::Idle => valve_controler.idle(),
            state::FilterState::ForcedFill(_) => valve_controler.fill(),
            state::FilterState::ForcedClean(_) => valve_controler.clean(),
            state::FilterState::ForcedIdle(_) => valve_controler.idle(),
        }
}

#[embassy_executor::task]
async fn measure_task(mut trig: Output<'static, PIN_17>, mut echo: Input<'static, PIN_16>) -> ! {
    loop {
        match measure(&mut trig, &mut echo) {
            Some(d) => {
                let mut c = STATE.lock().await;
                c.state.waterlevel = Some(d);
            },
            None => (),
        }
        Timer::after(Duration::from_secs(5)).await;
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