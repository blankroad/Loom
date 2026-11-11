//! Native device inventory; probes are non-promiscuous and handles close immediately.
use crate::{CaptureError, Result, live::map_error};
use pcap::{Capture, Device};
use std::net::IpAddr;
/// Reject excessive inventories rather than silently presenting an incomplete list.
pub const MAX_LIVE_INTERFACES: usize = 256;
/// Per-device address ceiling; reject excessive inventories before copying.
pub const MAX_INTERFACE_ADDRESSES: usize = 64;
/// Native description byte ceiling; reject rather than grow output without limit.
pub const MAX_INTERFACE_DESCRIPTION: usize = 4096;
/// Retained driver error text ceiling; truncate at a UTF-8 boundary and mark truncation.
pub const MAX_INTERFACE_REASON_BYTES: usize = 4096;
fn reason(error: CaptureError) -> String {
    let mut s = error.to_string();
    if s.len() > MAX_INTERFACE_REASON_BYTES {
        let mut n = MAX_INTERFACE_REASON_BYTES - 3;
        while !s.is_char_boundary(n) {
            n -= 1;
        }
        s.truncate(n);
        s.push_str("...");
    }
    s
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceInfo {
    pub name: String,
    pub desc: Option<String>,
    pub ips: Vec<IpAddr>,
    pub up: bool,
    pub loopback: bool,
    pub can_capture: bool,
    pub reason: Option<String>,
    pub permission_denied: bool,
}
fn describe(device: Device, probe: Result<()>) -> Result<InterfaceInfo> {
    if device.name.len() > loom_eth_pcapng::MAX_NAME_BYTES
        || device
            .desc
            .as_ref()
            .is_some_and(|s| s.len() > MAX_INTERFACE_DESCRIPTION)
        || device.addresses.len() > MAX_INTERFACE_ADDRESSES
    {
        return Err(CaptureError::Limit("device inventory"));
    }
    let permission_denied = matches!(&probe, Err(CaptureError::Permission(_)));
    Ok(InterfaceInfo {
        up: device.flags.is_up(),
        loopback: device.flags.is_loopback(),
        name: device.name,
        desc: device.desc,
        ips: device.addresses.into_iter().map(|a| a.addr).collect(),
        can_capture: probe.is_ok(),
        reason: probe.err().map(reason),
        permission_denied,
    })
}
/// Permission is observed by opening (snaplen 64, 1 ms timeout, promisc=false), then closing.
/// `can_capture` describes this instant; it does not guarantee a later open will succeed.
pub fn list_interfaces() -> Result<Vec<InterfaceInfo>> {
    let devices = Device::list().map_err(map_error)?;
    if devices.len() > MAX_LIVE_INTERFACES {
        return Err(CaptureError::Limit("device count"));
    }
    devices
        .into_iter()
        .map(|device| {
            let probe = Capture::from_device(device.name.as_str())
                .map_err(map_error)
                .and_then(|c| {
                    c.promisc(false)
                        .snaplen(64)
                        .timeout(1)
                        .open()
                        .map(drop)
                        .map_err(map_error)
                });
            describe(device, probe)
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    fn device() -> Device {
        Device {
            name: "en-test".into(),
            desc: Some("fixture".into()),
            addresses: vec![pcap::Address {
                addr: "192.0.2.1".parse().expect("literal"),
                netmask: None,
                broadcast_addr: None,
                dst_addr: None,
            }],
            flags: pcap::DeviceFlags::from(3),
        }
    }
    #[test]
    fn pure_inventory_permission_and_backend_verdicts() {
        let ok = describe(device(), Ok(())).expect("success");
        assert!(ok.can_capture && ok.up && ok.loopback);
        assert_eq!(ok.ips.len(), 1);
        let no = describe(
            device(),
            Err(map_error(pcap::Error::PcapError(
                "/dev/bpf: Permission denied".into(),
            ))),
        )
        .expect("denial");
        assert!(!no.can_capture && no.permission_denied);
        assert!(no.reason.expect("reason").contains("Permission"));
        let no =
            describe(device(), Err(CaptureError::Backend("device disappeared".into()))).expect("backend");
        assert!(!no.can_capture && !no.permission_denied);
        let mut big = device();
        big.addresses = vec![big.addresses[0].clone(); MAX_INTERFACE_ADDRESSES + 1];
        assert!(describe(big, Ok(())).is_err());
        let long =
            describe(device(), Err(CaptureError::Backend("권한".repeat(4096)))).expect("bounded reason");
        assert!(long.reason.expect("reason").len() <= MAX_INTERFACE_REASON_BYTES);
    }
    #[test]
    #[ignore = "opens native devices briefly; requires explicit human-configured capture permissions"]
    fn actual_inventory() {
        let list = list_interfaces().expect("inventory");
        assert!(list.len() <= MAX_LIVE_INTERFACES);
    }
}
