#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]
#![feature(async_closure)]

use thermo_esp::device_logic::ble;
use thermo_esp::hardware_concierge::thermo_control::ThermoControl;
use thermo_esp::device_logic::heater_controller;
use embassy_executor::Executor;
use esp_println::logger::init_logger;
use esp_wifi::initialize;
use embassy_executor::_export::StaticCell;
use esp_backtrace as _;
use examples_util::hal;
use hal::{
    clock::{ClockControl, CpuClock},
    embassy,
    peripherals::*,
    prelude::*,
    timer::TimerGroup,
    Rng, Rtc, IO,
};
use esp_wifi::EspWifiInitFor;
static EXECUTOR: StaticCell<Executor> = StaticCell::new();

#[entry]
fn main() -> ! {
    init_logger(log::LevelFilter::Info);
    let peripherals = Peripherals::take();

    let system = examples_util::system!(peripherals);
    let mut peripheral_clock_control = system.peripheral_clock_control;
    let clocks = examples_util::clocks!(system);
    examples_util::rtc!(peripherals);

    let timer = examples_util::timer!(peripherals, clocks, peripheral_clock_control);
    let init = initialize(
        EspWifiInitFor::Ble,
        timer,
        Rng::new(peripherals.RNG),
        system.radio_clock_control,
        &clocks,
    )
    .unwrap();

    // Async requires the GPIO interrupt to wake futures
    hal::interrupt::enable(
        hal::peripherals::Interrupt::GPIO,
        hal::interrupt::Priority::Priority1,
    )
    .unwrap();

    let bluetooth = examples_util::get_bluetooth!(peripherals);

    let timer_group0 = TimerGroup::new(peripherals.TIMG0, &clocks, &mut peripheral_clock_control);
    embassy::init(&clocks, timer_group0.timer0);

    let io = IO::new(peripherals.GPIO, peripherals.IO_MUX);
    let sclk = io.pins.gpio10;
    let cs = io.pins.gpio5;
    let miso = io.pins.gpio4;
    let toggler = io.pins.gpio8.into_push_pull_output();

    let thermo_control = ThermoControl::new(peripherals.SPI2, sclk, miso, cs, toggler, &mut peripheral_clock_control, &clocks);
    let executor = EXECUTOR.init(Executor::new());

    executor.run(|spawner| {
        spawner.spawn(ble::run(init, bluetooth)).unwrap();
        spawner.spawn(heater_controller::run(thermo_control)).unwrap();
    });
}
