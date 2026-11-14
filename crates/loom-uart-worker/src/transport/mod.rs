//! `transport` — byte transports for one UART port (UART plan §7.2, §7.3, §9.1).
//!
//! * [`SerialTransport`] — real ports and pty slaves (`serialport` 4.10.1 for open /
//!   termios / modem lines; on Unix a nonblocking fd with `poll` + partial I/O + absolute
//!   deadlines for data, §7.3).
//! * [`SimTransport`] — in-process fake device behind fixed-size buffers (`sim://<scenario>`),
//!   identical on all three OSes (Windows CI path).
//!
//! The reader thread and the TX actor run concurrently, so a transport hands out a read-only
//! [`RxHandle`] (a dup'd fd / cloned handle / shared buffer) while the TX actor keeps the
//! [`Transport`] itself (single owner of device settings, §7.3).

mod serial;
mod sim;

use std::fmt;
use std::io;
use std::time::{Duration, Instant};

#[cfg(unix)]
pub(crate) use serial::wait_fd;
pub use serial::{OpenOpts, SerialTransport};
pub use sim::{SIM_BUFFER_BYTES, SIM_SIGNAL_QUEUE, SimTransport};

/// Maximum number of ports returned by [`list_ports`] (UART plan §7.5: enumeration ≤ 256 ports).
pub const MAX_LISTED_PORTS: usize = 256;

/// Prefix selecting [`SimTransport`] in [`open`].
pub const SIM_SCHEME: &str = "sim://";

/// A byte transport for one port.
///
/// `read` returns `Ok(0)` only when no data arrived within `wait`; end of stream / unplug is an
/// error (`UnexpectedEof`, `BrokenPipe`, or the OS error). `write` never blocks past `deadline`
/// and reports partial progress.
pub trait Transport: Send + fmt::Debug {
    /// Reads up to `buf.len()` bytes, waiting at most `wait` for the first byte.
    fn read(&mut self, buf: &mut [u8], wait: Duration) -> io::Result<usize>;
    /// Writes as much of `buf` as the driver accepts before `deadline` (partial writes allowed).
    fn write(&mut self, buf: &[u8], deadline: Instant) -> WriteResult;
    /// Sets the DTR line. Errors are returned, never swallowed (§7.3).
    fn set_dtr(&mut self, on: bool) -> io::Result<()>;
    /// Sets the RTS line. Errors are returned, never swallowed (§7.3).
    fn set_rts(&mut self, on: bool) -> io::Result<()>;
    /// Asserts (`true`) or clears (`false`) a break condition. Timing is the caller's job (§7.2).
    fn set_break(&mut self, on: bool) -> io::Result<()>;
    /// The configuration the port was opened with.
    fn config(&self) -> &PortConfig;
    /// What kind of transport this is.
    fn kind(&self) -> TransportKind;
    /// Returns an independent read handle for the reader thread (§3.5, §7.3).
    fn rx_handle(&self) -> io::Result<Box<dyn RxHandle>>;
}

/// Read-only handle used by the reader thread, same semantics as [`Transport::read`].
pub trait RxHandle: Send + fmt::Debug {
    /// Reads up to `buf.len()` bytes, waiting at most `wait` for the first byte.
    fn read(&mut self, buf: &mut [u8], wait: Duration) -> io::Result<usize>;
}

/// Result of one [`Transport::write`] call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteResult {
    /// Bytes accepted by the driver (`sent` means "handed to the driver", not "on the wire").
    pub written: usize,
    /// Why the call ended.
    pub outcome: WriteOutcome,
}

/// How a [`Transport::write`] call ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOutcome {
    /// Every byte was accepted.
    Complete,
    /// The deadline passed with `written < buf.len()`.
    TimedOut,
    /// An I/O error ended the write.
    Failed(io::ErrorKind),
}

/// Transport family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransportKind {
    /// A real serial device.
    Serial,
    /// A pty slave opened through the real serial path. This identifies the transport, not
    /// internal SIM provenance; an external pty does not implicitly select the sim environment.
    Pty,
    /// In-process [`SimTransport`].
    Sim,
}

/// Parity setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Parity {
    /// No parity.
    #[default]
    None,
    /// Odd parity.
    Odd,
    /// Even parity.
    Even,
    /// Mark parity — not supported until the native extension (M2 T17, §7.2).
    Mark,
    /// Space parity — not supported until the native extension (M2 T17, §7.2).
    Space,
}

/// Stop bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum StopBits {
    /// One stop bit.
    #[default]
    One,
    /// 1.5 stop bits — not supported (Unix never; Windows after M2 T17, §7.2).
    OnePointFive,
    /// Two stop bits.
    Two,
}

/// Flow control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FlowControl {
    /// None.
    #[default]
    None,
    /// XON/XOFF.
    Software,
    /// RTS/CTS.
    Hardware,
}

/// Line settings for a port.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PortConfig {
    /// Baud rate (> 0).
    pub baud: u32,
    /// Data bits, 5..=8.
    pub data_bits: u8,
    /// Parity.
    pub parity: Parity,
    /// Stop bits.
    pub stop: StopBits,
    /// Flow control.
    pub flow: FlowControl,
}

impl Default for PortConfig {
    /// 115200 8N1, no flow control.
    fn default() -> Self {
        Self {
            baud: 115_200,
            data_bits: 8,
            parity: Parity::None,
            stop: StopBits::One,
            flow: FlowControl::None,
        }
    }
}

impl PortConfig {
    /// Checks the configuration. Mark/Space parity and 1.5 stop bits are rejected with
    /// `ErrorKind::Unsupported` — no silent substitution (§7.2).
    pub fn validate(&self) -> io::Result<()> {
        if self.baud == 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "baud must be > 0"));
        }
        if !(5..=8).contains(&self.data_bits) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("data_bits {} not in 5..=8", self.data_bits),
            ));
        }
        if matches!(self.parity, Parity::Mark | Parity::Space) {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                format!(
                    "parity {:?} is not supported yet (native extension, UART plan §7.2)",
                    self.parity
                ),
            ));
        }
        if self.stop == StopBits::OnePointFive {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "1.5 stop bits are not supported (UART plan §7.2)",
            ));
        }
        Ok(())
    }
}

/// Opens `target` for the worker binary.
///
/// `sim://<scenario>[?seed=N&...]` opens a [`SimTransport`]; anything else is a device path for
/// [`SerialTransport::open`]. **Only the worker binary calls this** — there is no CLI or UI path
/// that opens a device directly (contract 08 §8.3).
pub fn open(target: &str, config: &PortConfig, opts: &OpenOpts) -> io::Result<Box<dyn Transport>> {
    if target.starts_with(SIM_SCHEME) {
        let spec = crate::sim::SimSpec::parse(target)?;
        Ok(Box::new(SimTransport::open(&spec, config)?))
    } else {
        Ok(Box::new(SerialTransport::open(target, config, opts)?))
    }
}

/// How a listed port is attached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PortAttach {
    /// USB serial adapter.
    Usb,
    /// PCI / on-board port.
    Pci,
    /// Bluetooth serial.
    Bluetooth,
    /// Unknown.
    Unknown,
}

/// One enumerated port. USB fields are `None` for non-USB ports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortInfo {
    /// Device path (`/dev/ttyUSB0`, `COM3`, ...).
    pub path: String,
    /// Attachment type.
    pub attach: PortAttach,
    /// USB vendor id.
    pub vid: Option<u16>,
    /// USB product id.
    pub pid: Option<u16>,
    /// USB serial number.
    pub serial: Option<String>,
    /// USB manufacturer string.
    pub manufacturer: Option<String>,
    /// USB product string.
    pub product: Option<String>,
    /// USB interface number (communication interface on Windows/Linux, data on macOS — §7.2).
    pub interface: Option<u8>,
    /// USB location as `<bus>-<port>.<port>...`.
    pub location: Option<String>,
}

/// Result of [`list_ports`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PortList {
    /// At most [`MAX_LISTED_PORTS`] ports.
    pub ports: Vec<PortInfo>,
    /// `true` when the OS reported more ports than [`MAX_LISTED_PORTS`]; the list must not be
    /// used for automatic re-binding then (§7.5).
    pub truncated: bool,
}

/// Enumerates serial ports via `serialport` (sysfs on Linux, IOKit on macOS, SetupAPI on Windows).
pub fn list_ports() -> io::Result<PortList> {
    let found = serialport::available_ports().map_err(io::Error::from)?;
    Ok(port_list_from(found.into_iter().map(port_info_from)))
}

fn port_list_from(iter: impl Iterator<Item = PortInfo>) -> PortList {
    let mut list = PortList::default();
    for info in iter {
        if list.ports.len() >= MAX_LISTED_PORTS {
            list.truncated = true;
            break;
        }
        list.ports.push(info);
    }
    list
}

fn port_info_from(p: serialport::SerialPortInfo) -> PortInfo {
    let mut info = PortInfo {
        path: p.port_name,
        attach: PortAttach::Unknown,
        vid: None,
        pid: None,
        serial: None,
        manufacturer: None,
        product: None,
        interface: None,
        location: None,
    };
    match p.port_type {
        serialport::SerialPortType::UsbPort(u) => {
            info.attach = PortAttach::Usb;
            info.vid = Some(u.vid);
            info.pid = Some(u.pid);
            info.serial = u.serial_number;
            info.manufacturer = u.manufacturer;
            info.product = u.product;
            info.interface = u.interface;
            info.location = u.location.map(|l| {
                let chain: Vec<String> = l.port_chain().iter().map(u8::to_string).collect();
                format!("{}-{}", l.bus_id(), chain.join("."))
            });
        }
        serialport::SerialPortType::PciPort => info.attach = PortAttach::Pci,
        serialport::SerialPortType::BluetoothPort => info.attach = PortAttach::Bluetooth,
        serialport::SerialPortType::Unknown => {}
    }
    info
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        PortConfig::default().validate().expect("8N1 is valid");
    }

    #[test]
    fn mark_space_and_one_point_five_are_unsupported() {
        for parity in [Parity::Mark, Parity::Space] {
            let c = PortConfig {
                parity,
                ..PortConfig::default()
            };
            let e = c.validate().expect_err("mark/space must be rejected");
            assert_eq!(e.kind(), io::ErrorKind::Unsupported);
        }
        let c = PortConfig {
            stop: StopBits::OnePointFive,
            ..PortConfig::default()
        };
        let e = c.validate().expect_err("1.5 stop must be rejected");
        assert_eq!(e.kind(), io::ErrorKind::Unsupported);
    }

    #[test]
    fn bad_data_bits_and_baud_are_invalid() {
        for c in [
            PortConfig {
                data_bits: 9,
                ..PortConfig::default()
            },
            PortConfig {
                data_bits: 4,
                ..PortConfig::default()
            },
            PortConfig {
                baud: 0,
                ..PortConfig::default()
            },
        ] {
            let e = c.validate().expect_err("invalid config");
            assert_eq!(e.kind(), io::ErrorKind::InvalidInput);
        }
    }

    #[test]
    fn port_list_is_capped() {
        let infos = (0..(MAX_LISTED_PORTS + 5)).map(|i| {
            port_info_from(serialport::SerialPortInfo {
                port_name: format!("/dev/fake{i}"),
                port_type: serialport::SerialPortType::Unknown,
            })
        });
        let list = port_list_from(infos);
        assert_eq!(list.ports.len(), MAX_LISTED_PORTS);
        assert!(list.truncated);
    }

    #[test]
    fn usb_info_maps_all_fields() {
        let info = port_info_from(serialport::SerialPortInfo {
            port_name: "/dev/ttyUSB0".into(),
            port_type: serialport::SerialPortType::UsbPort(serialport::UsbPortInfo {
                vid: 0x0403,
                pid: 0x6001,
                serial_number: Some("A1".into()),
                manufacturer: Some("FTDI".into()),
                product: Some("FT232R".into()),
                location: Some(serialport::Location::new("1".into(), vec![2, 3])),
                interface: Some(0),
            }),
        });
        assert_eq!(info.attach, PortAttach::Usb);
        assert_eq!((info.vid, info.pid), (Some(0x0403), Some(0x6001)));
        assert_eq!(info.location.as_deref(), Some("1-2.3"));
        assert_eq!(info.interface, Some(0));
        assert_eq!(info.serial.as_deref(), Some("A1"));
    }

    #[test]
    fn list_ports_does_not_fail() {
        // Real enumeration: content is machine-specific, only the cap is asserted.
        let list = list_ports().expect("enumeration works without hardware");
        assert!(list.ports.len() <= MAX_LISTED_PORTS);
    }
}
