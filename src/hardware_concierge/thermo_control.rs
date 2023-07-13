use esp32c3_hal::gpio::{InputPin, OutputPin};
use esp32c3_hal::{
    peripherals::SPI2,
    gpio::NO_PIN,
    spi::{Spi, HalfDuplexMode, SpiMode, HalfDuplexReadWrite, SpiDataMode, Command, Address, Error}, system::PeripheralClockControl, clock::Clocks,
    prelude::_fugit_RateExtU32, 
    peripheral::Peripheral,
};
use esp_println::println;

pub struct ThermoControl<TG: OutputPin> {
    spi: Spi<'static, SPI2, HalfDuplexMode>,
    toggler: TG 
}

impl <TG: OutputPin> ThermoControl<TG>{
    pub fn new<
        SCK: OutputPin,
        MISO: OutputPin + InputPin,
        CS: OutputPin,
    >(
        spi: impl Peripheral<P = SPI2> + 'static,
        sclk: impl Peripheral<P = SCK> + 'static,
        miso: impl Peripheral<P = MISO> + 'static,
        cs: impl Peripheral<P = CS> + 'static,
        toggler: TG,
        peripheral_clock_control: &mut PeripheralClockControl,
        clocks: &Clocks,
    ) -> Self {
        let hd_spi = Spi::new_half_duplex(
            spi,
            Some(sclk),
            NO_PIN,
            Some(miso),
            NO_PIN,
            NO_PIN,
            Some(cs),
            4u32.MHz(),
            SpiMode::Mode0,
            peripheral_clock_control,
            clocks
        );

        ThermoControl {
            spi: hd_spi,
            toggler: toggler
        }
    }

    pub fn read_temperature(&mut self) -> Result<u16, Error> {
        let mut read_buffer: [u8; 2] = [0, 0];
        self.spi.read(
            SpiDataMode::Single,
            Command::None,
            Address::None,
            0,
            &mut read_buffer 
        )?;
        
        let casted_temp = u16::from_be_bytes(read_buffer);
        if ((casted_temp & 0x4) >> 2) == 1 {
            println!("Error during thermopare readout!");
            return Err(Error::Unknown);
        }
        Ok(((casted_temp & 0x7FF8) >> 3)/ 4)
    }

    pub fn set_heater_state(&mut self, state: bool) {
        let _pin = self.toggler.set_output_high(state);
    }
}

