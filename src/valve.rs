use embassy_rp::gpio::{Output, Pin};

pub struct Valve<T: Pin> {
    pin: Output<'static, T>,
}

impl<T: Pin> Valve<T> {
    fn open(&mut self) {
        self.pin.set_high();
    }

    fn close(&mut self) {
        self.pin.set_low();
    }

    pub const fn new(pin: Output<'static, T>) -> Self {
        Self { pin }
    }
}

pub struct ValveControler {
    valve1: Valve<embassy_rp::peripherals::PIN_12>,
    valve2: Valve<embassy_rp::peripherals::PIN_13>,
    valve3: Valve<embassy_rp::peripherals::PIN_14>,
    valve4: Valve<embassy_rp::peripherals::PIN_15>,
}

impl ValveControler {
    pub fn new(
        mut valve1: Valve<embassy_rp::peripherals::PIN_12>,
        mut valve2: Valve<embassy_rp::peripherals::PIN_13>,
        mut valve3: Valve<embassy_rp::peripherals::PIN_14>,
        mut valve4: Valve<embassy_rp::peripherals::PIN_15>,
    ) -> Self {
        valve1.close();
        valve2.close();
        valve3.close();
        valve4.close();

        Self {
            valve1,
            valve2,
            valve3,
            valve4,
        }
    }

    pub fn clean(&mut self) {
        self.valve1.open();
        self.valve2.open();
        self.valve3.open();
        self.valve4.close();
    }

    pub fn fill(&mut self) {
        self.valve1.open();
        self.valve2.open();
        self.valve3.close();
        self.valve4.open();
    }

    pub fn idle(&mut self) {
        self.valve1.close();
        self.valve2.close();
        self.valve3.close();
        self.valve4.close();
    }
}
