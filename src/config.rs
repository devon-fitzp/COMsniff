use serialport::{DataBits, Parity, StopBits};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Ascii,
}

impl Encoding {
    pub fn label(self) -> &'static str {
        match self {
            Encoding::Utf8 => "UTF-8",
            Encoding::Ascii => "ASCII",
        }
    }

    fn next(self) -> Self {
        match self {
            Encoding::Utf8 => Encoding::Ascii,
            Encoding::Ascii => Encoding::Utf8,
        }
    }
}

const BAUD_RATES: [u32; 8] = [1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200];
const DATA_BITS: [DataBits; 4] = [DataBits::Five, DataBits::Six, DataBits::Seven, DataBits::Eight];
const PARITIES: [Parity; 3] = [Parity::None, Parity::Odd, Parity::Even];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigField {
    Encoding,
    BaudRate,
    DataBits,
    StopBits,
    Parity,
}

impl ConfigField {
    pub const ALL: [ConfigField; 5] = [
        ConfigField::Encoding,
        ConfigField::BaudRate,
        ConfigField::DataBits,
        ConfigField::StopBits,
        ConfigField::Parity,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ConfigField::Encoding => "Encoding",
            ConfigField::BaudRate => "Baud Rate",
            ConfigField::DataBits => "Data Bits",
            ConfigField::StopBits => "Stop Bits",
            ConfigField::Parity => "Parity",
        }
    }

    pub fn next(self) -> Self {
        let idx = Self::ALL.iter().position(|f| *f == self).unwrap();
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    pub fn prev(self) -> Self {
        let idx = Self::ALL.iter().position(|f| *f == self).unwrap();
        Self::ALL[(idx + Self::ALL.len() - 1) % Self::ALL.len()]
    }
}

/// Handshake/flow-control (RTS/CTS, DTR/DSR) is intentionally not configurable yet.
/// The Config modal surfaces that as a static caption rather than omitting it silently.
#[derive(Debug, Clone)]
pub struct ConfigSettings {
    pub encoding: Encoding,
    pub baud_rate: u32,
    pub data_bits: DataBits,
    pub stop_bits: StopBits,
    pub parity: Parity,
}

impl Default for ConfigSettings {
    fn default() -> Self {
        Self {
            encoding: Encoding::Utf8,
            baud_rate: 9600,
            data_bits: DataBits::Eight,
            stop_bits: StopBits::One,
            parity: Parity::None,
        }
    }
}

impl ConfigSettings {
    pub fn cycle_field(&mut self, field: ConfigField, forward: bool) {
        match field {
            ConfigField::Encoding => self.encoding = self.encoding.next(),
            ConfigField::BaudRate => {
                self.baud_rate = cycle(&BAUD_RATES, self.baud_rate, forward, 3);
            }
            ConfigField::DataBits => {
                self.data_bits = cycle(&DATA_BITS, self.data_bits, forward, 3);
            }
            ConfigField::StopBits => {
                self.stop_bits = match self.stop_bits {
                    StopBits::One => StopBits::Two,
                    StopBits::Two => StopBits::One,
                };
            }
            ConfigField::Parity => {
                self.parity = cycle(&PARITIES, self.parity, forward, 0);
            }
        }
    }

    pub fn field_value_label(&self, field: ConfigField) -> String {
        match field {
            ConfigField::Encoding => self.encoding.label().to_string(),
            ConfigField::BaudRate => self.baud_rate.to_string(),
            ConfigField::DataBits => data_bits_label(self.data_bits).to_string(),
            ConfigField::StopBits => match self.stop_bits {
                StopBits::One => "1".to_string(),
                StopBits::Two => "2".to_string(),
            },
            ConfigField::Parity => parity_label(self.parity).to_string(),
        }
    }

    /// Compact "9600 8N1"-style summary used as a session-log header field.
    pub fn line_summary(&self) -> String {
        let parity_letter = match self.parity {
            Parity::None => 'N',
            Parity::Odd => 'O',
            Parity::Even => 'E',
        };
        let stop_bits = match self.stop_bits {
            StopBits::One => '1',
            StopBits::Two => '2',
        };
        format!("{} {}{parity_letter}{stop_bits}", self.baud_rate, data_bits_label(self.data_bits))
    }
}

fn cycle<T: PartialEq + Copy>(all: &[T], current: T, forward: bool, default_idx: usize) -> T {
    let idx = all.iter().position(|v| *v == current).unwrap_or(default_idx);
    let len = all.len();
    all[if forward { (idx + 1) % len } else { (idx + len - 1) % len }]
}

fn data_bits_label(bits: DataBits) -> &'static str {
    match bits {
        DataBits::Five => "5",
        DataBits::Six => "6",
        DataBits::Seven => "7",
        DataBits::Eight => "8",
    }
}

fn parity_label(parity: Parity) -> &'static str {
    match parity {
        Parity::None => "None",
        Parity::Odd => "Odd",
        Parity::Even => "Even",
    }
}
