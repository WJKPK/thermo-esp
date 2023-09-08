use crate::hardware_concierge::{ThermoToggler, thermo_control::ThermoControl};
use core::sync::atomic::{AtomicU16, Ordering};
use embassy_sync::signal::Signal;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_time::{Duration, Timer};
use heapless::Vec;
static SHARED: Signal<CriticalSectionRawMutex, RequestedMode> = Signal::new();
pub static TEMPERATURE_SIGNAL: Signal<CriticalSectionRawMutex, u16> = Signal::new();
static TEMPERATURE: AtomicU16 = AtomicU16::new(0);

#[derive(Clone, Debug)]
struct KeepTemperature {
    temperature: u16,
    histeresis: u16,
    expected_heating_duration_s: u32
}

impl KeepTemperature {
    fn decide_heater_state_change(&self, actual_temperature: u16) -> Option<bool> {
        log::info!("KeepTemperature: actual: {}, requested: {}", actual_temperature, self.temperature);
        if actual_temperature >= (self.temperature + self.histeresis) {
            Some(false)
        } else if actual_temperature < (self.temperature - self.histeresis) {
            Some(true)
        } else {
            None
        }
    }
}

#[derive(Clone, Debug)]
struct Heating {
    expected_temperature: u16,
    expected_heating_duration_s: u32
}

impl Heating {
    fn decide_heater_state(&self, shared_data: &StateMachineSharedData) -> bool {
        let d_temp = self.expected_temperature - shared_data.temperature_when_actual_state_start;
        let duration_of_state_time = shared_data.actual_time - shared_data.time_when_actual_state_start;

        let slope: u32 = (d_temp as u32/self.expected_heating_duration_s).into();
        let expected_temperature = (duration_of_state_time * slope) + shared_data.temperature_when_actual_state_start as u32;
        let actual_temperature = TEMPERATURE.load(Ordering::Relaxed);
        log::info!("Heating: actual: {} expected: {}", actual_temperature, expected_temperature);

        if u32::from(actual_temperature) > expected_temperature {
            false
        } else {
            true
        }
    }
}

#[derive(Clone, Debug)]
enum State {
    Off,
    Heating(Heating),
    KeepTemperature(KeepTemperature),
    Failure
}

struct StateMachineSharedData {
    thermo: ThermoControl<ThermoToggler>,
    temperature_when_actual_state_start: u16,
    time_when_actual_state_start: u32,
    actual_time: u32,
    actual_temperature: &'static AtomicU16
}

impl StateMachineSharedData {
    fn _set_current_temperature(&mut self) {
        let actual_temperature = self.thermo.read_temperature().unwrap_or(0);
        self.actual_temperature.store(actual_temperature, Ordering::Relaxed);
        TEMPERATURE_SIGNAL.signal(actual_temperature);
    }

    fn _update_time(&mut self) {
        self.actual_time += 1;
    }

    fn _set_state_start_temperature_and_time(&mut self) {
        let actual_temperature = self.actual_temperature.load(Ordering::Relaxed);
        self.temperature_when_actual_state_start = actual_temperature;
        self.time_when_actual_state_start = self.actual_time;
    }

    fn _reset_shared_data(&mut self) {
        self.actual_temperature.store(0, Ordering::Relaxed);
        self.temperature_when_actual_state_start = 0;
        self.actual_time = 0;
    }
}

struct HeaterStateMachine {
    shared_data: StateMachineSharedData,
    state: State
}

#[derive(Clone, Debug)]
enum Event {
    Off,
    Heating(Heating),
    KeepTemperature(KeepTemperature)
}

impl HeaterStateMachine {
    fn new(thermo: ThermoControl<ThermoToggler>) -> Self {
        let mut state_machine = Self {
            shared_data: StateMachineSharedData {
                temperature_when_actual_state_start: 0,
                thermo: thermo,
                actual_time: 0,
                time_when_actual_state_start: 0,
                actual_temperature: &TEMPERATURE
            },
            state: State::Off
        };
        state_machine.shared_data._set_current_temperature();
        state_machine.shared_data._set_state_start_temperature_and_time();
        state_machine
    }

    fn process_state_change(&mut self, event: Event) {
        match (&self.state, event) {
            (State::Off, Event::Off) => {
                self.state = State::Off;
            },
            (State::Off, Event::Heating(heating_parameters)) => {
                self.state = State::Heating(heating_parameters)
            },
            (State::Heating(_heating_parameters), Event::KeepTemperature(keep_temp_parameters)) => {
                self.state = State::KeepTemperature(keep_temp_parameters)
            },
            (State::KeepTemperature(_keep_temp_parameters), Event::Heating(heating_parameters)) => {
                self.state = State::Heating(heating_parameters)
            },
            (State::Heating(_heating_parameters), Event::Off) => {
                self.state = State::Off
            },
            (State::KeepTemperature(_heating_parameters), Event::Off) => {
                self.state = State::Off
            },
            (s, e) => {
                log::error!("Unsupported state change! While state {:?}, got event: {:?}!", s, e);
                self.state = State::Failure;
            }
        }
        self.shared_data._set_state_start_temperature_and_time();
    }

    fn run(&mut self) {
        self.shared_data._set_current_temperature();
        self.shared_data._update_time();

        match &self.state {
            State::Off => {
                self.shared_data.thermo.set_heater_state(false);
            },
            State::Heating(heating_parameters) => {
                let heater_state = heating_parameters.decide_heater_state(&self.shared_data);
                self.shared_data.thermo.set_heater_state(heater_state);
            },
            State::KeepTemperature(keep_temp_params) => {
                if let Some(state_to) = keep_temp_params.decide_heater_state_change(self.shared_data.actual_temperature.load(Ordering::Relaxed)) {
                    self.shared_data.thermo.set_heater_state(state_to);
                }
            },
            State::Failure => {
                self.shared_data.thermo.set_heater_state(false);
            }
        }
    }

    fn reset(&mut self) {
        self.state = State::Off;
        self.shared_data.thermo.set_heater_state(false);
        self.shared_data.actual_time = 0;
    }

    fn is_time_for_state_transition(&self, events_list: &[Event; 3]) -> Option<Event> {
        let actual_time = self.shared_data.actual_time;
        let mut rolling_sum = Vec::<u32, 4>::new();
        match rolling_sum.push(0) {
            Ok(_) => (),
            Err(_) => return Some(Event::Off),
        }

        let mut x: u32 = 0;
        for event in events_list.iter() {
            x += match event {
                Event::KeepTemperature(keep_temp_parameters) => keep_temp_parameters.expected_heating_duration_s,
                Event::Heating(heating_temp_parameters) => heating_temp_parameters.expected_heating_duration_s,
                Event::Off => 0
            };
            match rolling_sum.push(x) {
                Ok(_) => (),
                Err(_) => return Some(Event::Off),
            }
        }
        rolling_sum.truncate(events_list.len());
        for (i, time) in rolling_sum.iter().enumerate() {
            if *time == actual_time {
                if let Some(x) = events_list.get(i) {
                    return Some(x.clone());
                }
            }
        }
        if x < actual_time {
            return Some(Event::Off);
        }
        None
    }
}
#[derive(PartialEq)]
enum ReflowMode {
    JEDEC
}

#[derive(PartialEq)]
enum RequestedMode {
    Off,
    On(ReflowMode)
}


pub fn set_heating_profile(request_mode_from_ble: u8) {
    let state = match request_mode_from_ble {
        0 => RequestedMode::Off,
        1 => RequestedMode::On(ReflowMode::JEDEC),
        2_u8..=u8::MAX => RequestedMode::Off,
    };
    SHARED.signal(state);
}

pub fn read_temperature() -> u16 {
    TEMPERATURE.load(Ordering::Relaxed)
}

#[embassy_executor::task]
pub async fn run(thermo: ThermoControl<ThermoToggler>) {
    let mut state_machine = HeaterStateMachine::new(thermo);
    let jedec_events = [Event::Heating(Heating {
            expected_temperature: 100,
            expected_heating_duration_s: 60
        }),
        Event::KeepTemperature(KeepTemperature {
                temperature: 100,
                histeresis: 5,
                expected_heating_duration_s: 60
        }),
        Event::Off
    ];
    loop {
        let requested_state = SHARED.wait().await;
        loop {
            if requested_state == RequestedMode::Off {
                state_machine.reset();
                break;
            }

            if let Some(event) = state_machine.is_time_for_state_transition(&jedec_events) {
                state_machine.process_state_change(event);
            }

            state_machine.run();

            if SHARED.signaled() {
                break;
            }
            Timer::after(Duration::from_secs(1)).await;
        }
    }
}

