pub mod thermo_control;

pub(crate) type ThermoToggler = esp32c3_hal::gpio::Gpio8<esp32c3_hal::gpio::Output<esp32c3_hal::gpio::PushPull>>;
