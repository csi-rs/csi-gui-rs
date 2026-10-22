//! Finding ESP32 boards among the OS serial ports.
//!
//! A port is a likely ESP32 when its USB vendor is Espressif (native
//! USB-Serial-JTAG on the S3 / C3 / C5 / C6) or one of the USB-UART bridges
//! the dev boards carry (Silicon Labs CP210x, WCH CH340/CH9102). Ports with
//! no USB information fall back to their name.
//!
//! The USB serial number matters: on native USB it is the chip's MAC, so it
//! names the board even before the firmware answers, and it lets a board
//! that re-enumerated under a new path (after a `restart`, or a replug into
//! another socket) be followed instead of treated as a new one.

/// Espressif (native USB-Serial-JTAG).
pub const VID_ESPRESSIF: u16 = 0x303A;
/// Silicon Labs CP210x.
pub const VID_SILABS: u16 = 0x10C4;
/// WCH CH340 / CH9102.
pub const VID_WCH: u16 = 0x1A86;
/// FTDI, on a few older boards.
pub const VID_FTDI: u16 = 0x0403;

/// One serial port as the OS lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortCandidate {
    /// OS name: `/dev/ttyACM0`, `COM3`, `/dev/cu.usbmodem…`.
    pub name: String,
    /// USB vendor id, when it is a USB port.
    pub vid: Option<u16>,
    /// USB product id.
    pub pid: Option<u16>,
    /// USB serial number. The chip MAC on native USB.
    pub serial: Option<String>,
    /// Product string, e.g. `USB JTAG/serial debug unit`.
    pub product: Option<String>,
}

impl PortCandidate {
    /// `true` when this looks like an ESP32 (see the module docs).
    pub fn is_likely_esp(&self) -> bool {
        match self.vid {
            Some(v) => matches!(v, VID_ESPRESSIF | VID_SILABS | VID_WCH | VID_FTDI),
            None => {
                let n = self.name.to_ascii_lowercase();
                ["ttyusb", "ttyacm", "usbserial", "usbmodem", "wchusbserial", "slab_usbtouart"]
                    .iter()
                    .any(|p| n.contains(p))
            }
        }
    }

    /// `true` for the chip's own USB-Serial-JTAG, where DTR/RTS can't reset it.
    pub fn is_native_usb(&self) -> bool {
        self.vid == Some(VID_ESPRESSIF)
    }

    /// Short description for menus: `ttyACM0 · Espressif USB-JTAG`.
    pub fn describe(&self) -> String {
        let short = self.name.rsplit('/').next().unwrap_or(&self.name);
        let kind = match self.vid {
            Some(VID_ESPRESSIF) => "Espressif USB-JTAG",
            Some(VID_SILABS) => "CP210x",
            Some(VID_WCH) => "CH340",
            Some(VID_FTDI) => "FTDI",
            Some(_) => "USB serial",
            None => "serial",
        };
        format!("{short} · {kind}")
    }

    /// The board identity a USB serial number gives away, normalised to
    /// `AA:BB:CC:DD:EE:FF`. Only native-USB serials are MACs.
    pub fn mac_hint(&self) -> Option<String> {
        if !self.is_native_usb() {
            return None;
        }
        normalize_mac(self.serial.as_deref()?)
    }
}

/// List every serial port, ESP32-looking ones first.
pub fn scan() -> Vec<PortCandidate> {
    let mut out: Vec<PortCandidate> = serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .map(|p| match p.port_type {
            serialport::SerialPortType::UsbPort(u) => PortCandidate {
                name: p.port_name,
                vid: Some(u.vid),
                pid: Some(u.pid),
                serial: u.serial_number,
                product: u.product,
            },
            _ => PortCandidate {
                name: p.port_name,
                vid: None,
                pid: None,
                serial: None,
                product: None,
            },
        })
        // macOS lists every device twice, as `tty.*` and `cu.*`; only the
        // `cu.*` side opens without waiting for carrier detect.
        .filter(|p| !p.name.starts_with("/dev/tty.") || cfg!(not(target_os = "macos")))
        .collect();
    out.sort_by_key(|p| (!p.is_likely_esp(), p.name.clone()));
    out
}

/// `AA:BB:CC:DD:EE:FF` from any of `aabbccddeeff`, `aa:bb:…`, `AA-BB-…`.
/// `None` unless there are exactly 12 hex digits.
pub fn normalize_mac(s: &str) -> Option<String> {
    let hex: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if hex.len() != 12 || s.chars().any(|c| !(c.is_ascii_hexdigit() || matches!(c, ':' | '-'))) {
        return None;
    }
    let hex = hex.to_ascii_uppercase();
    Some(
        (0..6)
            .map(|i| &hex[i * 2..i * 2 + 2])
            .collect::<Vec<_>>()
            .join(":"),
    )
}

/// Where a board known by `serial` (a USB serial number) went: the new
/// port name, when it shows up under a path other than `old_name`.
pub fn find_moved<'a>(ports: &'a [PortCandidate], serial: &str, old_name: &str) -> Option<&'a str> {
    ports
        .iter()
        .find(|p| p.name != old_name && p.serial.as_deref() == Some(serial))
        .map(|p| p.name.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn port(name: &str, vid: Option<u16>, serial: Option<&str>) -> PortCandidate {
        PortCandidate {
            name: name.into(),
            vid,
            pid: None,
            serial: serial.map(str::to_string),
            product: None,
        }
    }

    #[test]
    fn recognises_esp_ports() {
        assert!(port("/dev/ttyACM0", Some(VID_ESPRESSIF), None).is_likely_esp());
        assert!(port("COM7", Some(VID_WCH), None).is_likely_esp());
        assert!(!port("/dev/ttyACM9", Some(0x2341), None).is_likely_esp()); // Arduino
        assert!(port("/dev/ttyUSB3", None, None).is_likely_esp());
        assert!(!port("/dev/ttyS0", None, None).is_likely_esp());
    }

    #[test]
    fn native_usb_serial_is_the_mac() {
        let p = port("/dev/ttyACM0", Some(VID_ESPRESSIF), Some("a0:85:e3:f5:48:90"));
        assert_eq!(p.mac_hint().as_deref(), Some("A0:85:E3:F5:48:90"));
        let bridge = port("/dev/ttyUSB0", Some(VID_SILABS), Some("0001"));
        assert_eq!(bridge.mac_hint(), None);
    }

    #[test]
    fn normalizes_mac_spellings() {
        assert_eq!(normalize_mac("a085e3f54890").as_deref(), Some("A0:85:E3:F5:48:90"));
        assert_eq!(normalize_mac("A0-85-E3-F5-48-90").as_deref(), Some("A0:85:E3:F5:48:90"));
        assert_eq!(normalize_mac("0001"), None);
        assert_eq!(normalize_mac("zz:85:e3:f5:48:90"), None);
    }

    #[test]
    fn follows_a_reenumerated_board() {
        let ports = vec![
            port("/dev/ttyACM1", Some(VID_ESPRESSIF), Some("AA")),
            port("/dev/ttyACM3", Some(VID_ESPRESSIF), Some("BB")),
        ];
        assert_eq!(find_moved(&ports, "BB", "/dev/ttyACM0"), Some("/dev/ttyACM3"));
        assert_eq!(find_moved(&ports, "AA", "/dev/ttyACM1"), None);
        assert_eq!(find_moved(&ports, "CC", "/dev/ttyACM0"), None);
    }
}
