use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use simple_endian::BigEndian;
use spin::RwLock;

use super::ip::{Ipv4, Ipv4Addr};
use super::{checksum, ip, PacketDownHierarchy};

use super::{ConstPacketKind, Packet, PacketHeader, PacketUpHierarchy};

#[derive(Copy, Clone)]
pub struct Udp;

impl ConstPacketKind for Udp {
    const HSIZE: usize = core::mem::size_of::<Header>();
}

impl Packet<Udp> {
    pub fn create(src_port: u16, dest_port: u16, mut size: usize, target: Ipv4Addr) -> Packet<Udp> {
        size += Udp::HSIZE;

        let ip_packet = Packet::<Ipv4>::create(ip::Type::Udp, target, size);
        let mut packet: Packet<Udp> = ip_packet.upgrade();

        let header = packet.header_mut();

        header.src_port = BigEndian::from(src_port);
        header.dst_port = BigEndian::from(dest_port);
        header.len = BigEndian::from(size as u16);

        packet
    }
}

impl PacketUpHierarchy<Udp> for Packet<Ipv4> {}
impl PacketHeader<Header> for Packet<Udp> {
    fn send(&self) {
        {
            let mut this = self.clone();
            let header = this.header_mut();
            header.compute_checksum(self.downgrade().header());
        }

        self.downgrade().send() // send the IP packet
    }

    fn recv(&self) {
        let header = self.header();
        let dest_port = header.dst_port().to_native();

        let handlers = HANDLERS.read();

        if let Some(handler) = handlers.get(&dest_port) {
            handler.recv(self.clone());
        } else {
            log::warn!("udp: no handler registered for port {}", dest_port);
        }
    }
}

#[repr(C, packed)]
pub struct Header {
    pub src_port: BigEndian<u16>,
    pub dst_port: BigEndian<u16>,
    pub len: BigEndian<u16>,
    pub crc: BigEndian<u16>,
}

impl Header {
    fn compute_checksum(&mut self, header: &ip::Header) {
        self.crc = BigEndian::from(0);

        let length = self.len;
        self.crc = checksum::make_combine(&[
            checksum::calculate(&checksum::PseudoHeader::new(header)),
            checksum::calculate_with_len(self, length.to_native() as usize),
        ]);
    }

    fn dst_port(&self) -> BigEndian<u16> {
        self.dst_port
    }
}

static HANDLERS: RwLock<BTreeMap<u16, Arc<dyn UdpHandler>>> = RwLock::new(BTreeMap::new());

pub trait UdpHandler: Send + Sync {
    fn recv(&self, packet: Packet<Udp>);
}

pub fn alloc_ephemeral_port(socket: Arc<dyn UdpHandler>) -> Option<u16> {
    const EPHEMERAL_START: u16 = 49152;
    const EPHEMERAL_END: u16 = u16::MAX;

    let mut handlers = HANDLERS.write();

    // Ephemeral ports in the range 49152..65535 are not
    // assigned, controlled, or registered and are used
    // for temporary or private ports.
    for port in EPHEMERAL_START..=EPHEMERAL_END {
        if handlers.contains_key(&port) {
            continue;
        }

        handlers.insert(port, socket);
        return Some(port);
    }

    None
}

pub fn bind(port: u16, socket: Arc<dyn UdpHandler>) {
    log::trace!("udp: bind(port={port})");

    let mut handlers = HANDLERS.write();
    // check if the port is already in use
    assert!(!handlers.contains_key(&port));

    handlers.insert(port, socket);
}

pub fn connect(host: Ipv4Addr, port: u16) {
    log::trace!("udp: connect(host={host:?}, port={port})");
}
