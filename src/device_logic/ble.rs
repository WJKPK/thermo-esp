use crate::device_logic::heater_controller::{TEMPERATURE_SIGNAL,read_temperature, set_heating_profile};
use bleps::{
    ad_structure::{
        create_advertising_data, AdStructure, BR_EDR_NOT_SUPPORTED, LE_GENERAL_DISCOVERABLE,
    },
    async_attribute_server::AttributeServer,
    asynch::Ble,
    attribute_server::NotificationData,
    gatt, };
use esp_backtrace as _;
use esp_println::println;
use esp_wifi::{
    ble::controller::asynch::BleConnector, EspWifiInitialization,
};
use esp32c3_hal::radio::Bluetooth;

pub struct BluetoothCallbacks {
    pub temp_read: fn(usize, &mut [u8]) -> usize,
    pub profile_setup: fn(usize, &[u8]),
}

impl BluetoothCallbacks {
    pub fn new(temp_read: fn(usize, &mut [u8]) -> usize, profile_setup: fn(usize, &[u8])) -> Self {
        Self { temp_read, profile_setup}
    }
}

unsafe fn any_as_u8_slice<T: Sized>(p: &T) -> &[u8] {
    ::core::slice::from_raw_parts(
        (p as *const T) as *const u8,
        ::core::mem::size_of::<T>(),
    )
}

#[embassy_executor::task]
pub async fn run(init: EspWifiInitialization, mut bluetooth: Bluetooth) {
    let connector = BleConnector::new(&init, &mut bluetooth);
    let mut ble = Ble::new(connector, esp_wifi::current_millis);
    println!("Connector created");

    loop {
        println!("{:?}", ble.init().await);
        println!("{:?}", ble.cmd_set_le_advertising_parameters().await);
        println!(
            "{:?}",
            ble.cmd_set_le_advertising_data(
                create_advertising_data(&[
                    AdStructure::Flags(LE_GENERAL_DISCOVERABLE | BR_EDR_NOT_SUPPORTED),
                    AdStructure::ServiceUuids16(&[Uuid::Uuid16(0x1809)]),
                    AdStructure::CompleteLocalName(examples_util::SOC_NAME),
                ])
                .unwrap()
            )
            .await
        );
        println!("{:?}", ble.cmd_set_le_advertise_enable(true).await);

        println!("started advertising");

        let mut rf = |_offset: usize, data: &mut[u8]| -> usize {
            let temperature: u16 = read_temperature();
            let temperature: &[u8] = unsafe { any_as_u8_slice(&temperature)};
            data[0..2].copy_from_slice(temperature);
            ::core::mem::size_of::<u16>()
        };
        let mut wf2 = |_offset: usize, data: &[u8]| {
            set_heating_profile(data[0]);
        };

        gatt!([service {
            uuid: "937312e0-2354-11eb-9f10-fbc30a62cf38",
            characteristics: [
                characteristic { uuid: "937312e0-2354-11eb-9f10-fbc30a62cf38",
                    name: "temperature",
                    read: rf,
                    notify: true,
                },
                characteristic {
                    uuid: "957312e0-2354-11eb-9f10-fbc30a62cf38",
                    write: wf2,
                }
            ],
        },]);

        let mut srv = AttributeServer::new(&mut ble, &mut gatt_attributes);
        let mut notifier = async || {
            let temperature = TEMPERATURE_SIGNAL.wait().await;
            let mut data = [0u8; 2];
            let temperature: &[u8] = unsafe { any_as_u8_slice(&temperature)};
            data.copy_from_slice(temperature);
            NotificationData::new(temperature_handle, &data)
        };
        
        srv.run(&mut notifier).await.unwrap();
    }
}

