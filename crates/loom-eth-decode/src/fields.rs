//! Field registry: every field the decoder can emit, with type, display base and aliases.
//!
//! Canonical names follow Wireshark display-filter names (plan §4.1, decision X4). Names not yet
//! checked against `tshark -G fields` are listed in the P0 report as "Wireshark 대조 필요".
//! Union fields (`ip.addr`, `tcp.port` …) are not emitted; they match any of their members.

use serde::Serialize;

use crate::value::{Base, FieldType};

/// Definition of one filterable field.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct FieldDef {
    /// Canonical name (Wireshark spelling).
    pub name: &'static str,
    pub ty: FieldType,
    pub base: Base,
    pub desc: &'static str,
    /// Other spellings accepted by the filter (old Wireshark names, snake_case variants).
    pub aliases: &'static [&'static str],
    /// If non-empty, this field is a union of the listed member fields (multi-value "any").
    pub union: &'static [&'static str],
}

const fn d(name: &'static str, ty: FieldType, base: Base, desc: &'static str) -> FieldDef {
    FieldDef {
        name,
        ty,
        base,
        desc,
        aliases: &[],
        union: &[],
    }
}
const fn da(
    name: &'static str,
    ty: FieldType,
    base: Base,
    desc: &'static str,
    aliases: &'static [&'static str],
) -> FieldDef {
    FieldDef {
        name,
        ty,
        base,
        desc,
        aliases,
        union: &[],
    }
}
const fn du(
    name: &'static str,
    ty: FieldType,
    desc: &'static str,
    union: &'static [&'static str],
) -> FieldDef {
    FieldDef {
        name,
        ty,
        base: Base::Dec,
        desc,
        aliases: &[],
        union,
    }
}
const fn p(name: &'static str, desc: &'static str) -> FieldDef {
    FieldDef {
        name,
        ty: FieldType::Protocol,
        base: Base::Hex,
        desc,
        aliases: &[],
        union: &[],
    }
}

use Base::{Dec, Hex};
use FieldType::{Bool, Bytes, Ipv4, Ipv6, Mac, Str, TimeRel, U8, U16, U32, U64};

/// All fields. Order = tree order inside each protocol.
pub static FIELDS: &[FieldDef] = &[
    // frame / loom
    p("frame", "Frame"),
    d("frame.len", U32, Dec, "Frame length on the wire"),
    d("frame.cap_len", U32, Dec, "Captured length"),
    d(
        "frame.time_relative",
        TimeRel,
        Dec,
        "Seconds since the filter's time reference",
    ),
    d(
        "frame.time_delta",
        TimeRel,
        Dec,
        "Seconds since the previous evaluated frame",
    ),
    p("loom", "Loom metadata"),
    d("loom.seq", U64, Dec, "Loom source sequence number"),
    d("loom.dir", Str, Dec, "Direction seen by the NIC: rx | tx"),
    d(
        "loom.origin",
        Str,
        Dec,
        "Attributed origin: agent | user | script, else device",
    ),
    d("loom.request_id", Str, Dec, "Linked request id (UDS or eth.send)"),
    d(
        "loom.pdu_first_seq",
        U64,
        Dec,
        "Loom seq of the first segment of a reassembled PDU",
    ),
    // link
    p("null", "BSD loopback"),
    d("null.family", U32, Dec, "Address family"),
    p("sll", "Linux cooked capture"),
    d("sll.pkttype", U16, Dec, "Packet type"),
    d("sll.hatype", U16, Dec, "Link-layer address type"),
    d("sll.etype", U16, Hex, "Protocol"),
    p("eth", "Ethernet II"),
    d("eth.dst", Mac, Hex, "Destination"),
    d("eth.src", Mac, Hex, "Source"),
    du("eth.addr", Mac, "Source or destination", &["eth.src", "eth.dst"]),
    d("eth.type", U16, Hex, "EtherType"),
    p("vlan", "802.1Q VLAN"),
    d("vlan.priority", U8, Dec, "Priority (PCP)"),
    d("vlan.dei", Bool, Dec, "Drop eligible"),
    d("vlan.id", U16, Dec, "VLAN id"),
    d("vlan.etype", U16, Hex, "EtherType"),
    p("arp", "ARP"),
    d("arp.hw.type", U16, Dec, "Hardware type"),
    d("arp.proto.type", U16, Hex, "Protocol type"),
    d("arp.opcode", U16, Dec, "Opcode"),
    d("arp.src.hw_mac", Mac, Hex, "Sender MAC"),
    d("arp.src.proto_ipv4", Ipv4, Dec, "Sender IP"),
    d("arp.dst.hw_mac", Mac, Hex, "Target MAC"),
    d("arp.dst.proto_ipv4", Ipv4, Dec, "Target IP"),
    // network
    p("ip", "IPv4"),
    d("ip.version", U8, Dec, "Version"),
    d("ip.hdr_len", U8, Dec, "Header length (bytes)"),
    d("ip.dsfield", U8, Hex, "DS field"),
    d("ip.len", U16, Dec, "Total length"),
    d("ip.id", U16, Hex, "Identification"),
    d("ip.flags.df", Bool, Dec, "Don't fragment"),
    d("ip.flags.mf", Bool, Dec, "More fragments"),
    d("ip.frag_offset", U16, Dec, "Fragment offset (bytes)"),
    d("ip.ttl", U8, Dec, "Time to live"),
    d("ip.proto", U8, Dec, "Protocol"),
    d("ip.checksum", U16, Hex, "Header checksum"),
    d("ip.src", Ipv4, Dec, "Source"),
    d("ip.dst", Ipv4, Dec, "Destination"),
    du("ip.addr", Ipv4, "Source or destination", &["ip.src", "ip.dst"]),
    p("ipv6", "IPv6"),
    d("ipv6.version", U8, Dec, "Version"),
    d("ipv6.tclass", U8, Hex, "Traffic class"),
    d("ipv6.flow", U32, Hex, "Flow label"),
    d("ipv6.plen", U16, Dec, "Payload length"),
    d("ipv6.nxt", U8, Dec, "Next header"),
    d("ipv6.hlim", U8, Dec, "Hop limit"),
    d("ipv6.src", Ipv6, Dec, "Source"),
    d("ipv6.dst", Ipv6, Dec, "Destination"),
    du(
        "ipv6.addr",
        Ipv6,
        "Source or destination",
        &["ipv6.src", "ipv6.dst"],
    ),
    p("icmp", "ICMP"),
    d("icmp.type", U8, Dec, "Type"),
    d("icmp.code", U8, Dec, "Code"),
    d("icmp.checksum", U16, Hex, "Checksum"),
    d("icmp.ident", U16, Dec, "Echo identifier"),
    d("icmp.seq", U16, Dec, "Echo sequence number"),
    p("icmpv6", "ICMPv6"),
    d("icmpv6.type", U8, Dec, "Type"),
    d("icmpv6.code", U8, Dec, "Code"),
    d("icmpv6.checksum", U16, Hex, "Checksum"),
    d("icmpv6.echo.identifier", U16, Dec, "Echo identifier"),
    d("icmpv6.echo.sequence_number", U16, Dec, "Echo sequence number"),
    // transport
    p("udp", "UDP"),
    d("udp.srcport", U16, Dec, "Source port"),
    d("udp.dstport", U16, Dec, "Destination port"),
    du(
        "udp.port",
        U16,
        "Source or destination port",
        &["udp.srcport", "udp.dstport"],
    ),
    d("udp.length", U16, Dec, "Length"),
    d("udp.checksum", U16, Hex, "Checksum"),
    p("tcp", "TCP"),
    d("tcp.srcport", U16, Dec, "Source port"),
    d("tcp.dstport", U16, Dec, "Destination port"),
    du(
        "tcp.port",
        U16,
        "Source or destination port",
        &["tcp.srcport", "tcp.dstport"],
    ),
    d("tcp.stream", U32, Dec, "Stream index"),
    d("tcp.len", U32, Dec, "Segment payload length"),
    d("tcp.seq", U32, Dec, "Relative sequence number"),
    d("tcp.seq_raw", U32, Dec, "Sequence number (raw)"),
    d("tcp.ack", U32, Dec, "Relative acknowledgment number"),
    d("tcp.ack_raw", U32, Dec, "Acknowledgment number (raw)"),
    d("tcp.hdr_len", U8, Dec, "Header length (bytes)"),
    d("tcp.flags", U16, Hex, "Flags"),
    d("tcp.flags.fin", Bool, Dec, "FIN"),
    d("tcp.flags.syn", Bool, Dec, "SYN"),
    da("tcp.flags.reset", Bool, Dec, "RST", &["tcp.flags.rst"]),
    da("tcp.flags.push", Bool, Dec, "PSH", &["tcp.flags.psh"]),
    d("tcp.flags.ack", Bool, Dec, "ACK"),
    d("tcp.flags.urg", Bool, Dec, "URG"),
    da(
        "tcp.window_size_value",
        U16,
        Dec,
        "Window (unscaled)",
        &["tcp.window"],
    ),
    d("tcp.checksum", U16, Hex, "Checksum"),
    d(
        "tcp.analysis.retransmission",
        Bool,
        Dec,
        "All payload already seen",
    ),
    d("tcp.analysis.out_of_order", Bool, Dec, "Segment filled a hole"),
    d(
        "tcp.analysis.lost_segment",
        Bool,
        Dec,
        "Previous segment not captured",
    ),
    // DoIP (temporary decoder, see doip.rs)
    p("doip", "Diagnostics over IP"),
    d("doip.version", U8, Hex, "Protocol version"),
    d("doip.inverse_version", U8, Hex, "Inverse protocol version"),
    da("doip.type", U16, Hex, "Payload type", &["doip.payload_type"]),
    d("doip.length", U32, Dec, "Payload length"),
    d("doip.source_address", U16, Hex, "Source address"),
    d("doip.target_address", U16, Hex, "Target address"),
    d("doip.logical_address", U16, Hex, "Logical address"),
    d("doip.vin", Str, Dec, "VIN"),
    d("doip.eid", Bytes, Hex, "EID"),
    d("doip.gid", Bytes, Hex, "GID"),
    da(
        "doip.further_action",
        U8,
        Hex,
        "Further action required",
        &["doip.futher_action"],
    ),
    d("doip.vin_gid_sync", U8, Hex, "VIN/GID sync status"),
    d("doip.activation_type", U8, Hex, "Routing activation type"),
    d("doip.response_code", U8, Hex, "Routing activation response code"),
    d("doip.ack_code", U8, Hex, "Diagnostic ACK code"),
    d("doip.nack_code", U8, Hex, "Diagnostic NACK code"),
    d("doip.header_nack_code", U8, Hex, "Generic header NACK code"),
    d("doip.node_type", U8, Hex, "Node type"),
    d("doip.max_sockets", U8, Dec, "Max concurrent sockets"),
    d("doip.current_sockets", U8, Dec, "Currently open sockets"),
    d("doip.max_data_size", U32, Dec, "Max data size"),
    d("doip.power_mode", U8, Hex, "Diagnostic power mode"),
    // UDS (temporary decoder, see uds.rs)
    p("uds", "Unified Diagnostic Services"),
    d("uds.sid", U8, Hex, "Service id (first byte)"),
    d("uds.reply", Bool, Dec, "Response (positive or negative)"),
    d("uds.subfunction", U8, Hex, "Sub-function (without suppress bit)"),
    d("uds.did", U16, Hex, "Data identifier"),
    d("uds.rid", U16, Hex, "Routine identifier"),
    d("uds.err.sid", U8, Hex, "Rejected service id"),
    da("uds.nrc", U8, Hex, "Negative response code", &["uds.err.code"]),
    d("uds.dtc_count", U32, Dec, "DTC records in a 59 02 response"),
    // SOME/IP
    p("someip", "SOME/IP"),
    d("someip.messageid", U32, Hex, "Message id"),
    da("someip.serviceid", U16, Hex, "Service id", &["someip.service_id"]),
    da(
        "someip.methodid",
        U16,
        Hex,
        "Method / event id",
        &["someip.method_id"],
    ),
    d("someip.length", U32, Dec, "Length"),
    da("someip.clientid", U16, Hex, "Client id", &["someip.client_id"]),
    da("someip.sessionid", U16, Hex, "Session id", &["someip.session_id"]),
    da(
        "someip.protoversion",
        U8,
        Dec,
        "Protocol version",
        &["someip.protocol_version"],
    ),
    da(
        "someip.interfaceversion",
        U8,
        Dec,
        "Interface version",
        &["someip.interface_version"],
    ),
    da(
        "someip.messagetype",
        U8,
        Hex,
        "Message type",
        &["someip.message_type"],
    ),
    da(
        "someip.returncode",
        U8,
        Hex,
        "Return code",
        &["someip.return_code"],
    ),
    d("someip.payload", Bytes, Hex, "Payload"),
    p("someipsd", "SOME/IP Service Discovery"),
    d("someipsd.flags", U8, Hex, "Flags"),
    d("someipsd.flags.reboot", Bool, Dec, "Reboot flag"),
    d("someipsd.flags.unicast", Bool, Dec, "Unicast flag"),
    d("someipsd.length_entriesarray", U32, Dec, "Entries array length"),
    d("someipsd.entry.type", U8, Hex, "Entry type"),
    da(
        "someipsd.entry.serviceid",
        U16,
        Hex,
        "Service id",
        &["someipsd.entry.service_id"],
    ),
    da(
        "someipsd.entry.instanceid",
        U16,
        Hex,
        "Instance id",
        &["someipsd.entry.instance_id"],
    ),
    d("someipsd.entry.majorver", U8, Dec, "Major version"),
    d("someipsd.entry.ttl", U32, Dec, "TTL"),
    d("someipsd.entry.minorver", U32, Dec, "Minor version"),
    d("someipsd.entry.eventgroupid", U16, Hex, "Eventgroup id"),
    d("someipsd.length_optionsarray", U32, Dec, "Options array length"),
    d("someipsd.option.type", U8, Hex, "Option type"),
    d("someipsd.option.ipv4", Ipv4, Dec, "Endpoint IPv4 address"),
    d("someipsd.option.ipv6", Ipv6, Dec, "Endpoint IPv6 address"),
    d("someipsd.option.proto", U8, Dec, "Endpoint L4 protocol"),
    d("someipsd.option.port", U16, Dec, "Endpoint port"),
];

/// Looks up a field by canonical name or alias.
pub fn field_def(name: &str) -> Option<&'static FieldDef> {
    FIELDS
        .iter()
        .find(|f| f.name == name || f.aliases.contains(&name))
}

/// Protocol names (presence fields), in registry order.
pub fn protocols() -> impl Iterator<Item = &'static str> {
    FIELDS
        .iter()
        .filter(|f| f.ty == FieldType::Protocol)
        .map(|f| f.name)
}
