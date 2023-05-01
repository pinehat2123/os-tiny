use core::alloc::{Allocator, Layout};

extern crate dma as d;
extern crate mem as m;

use d::DmaAllocator;
use m::paging::{PageSize, Size4KiB};

use super::{ConstPacketKind, Packet};

#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Default)]
#[repr(transparent)]
pub struct MacAddr(pub [u8; Self::ADDR_SIZE]);

impl MacAddr {
    pub const ADDR_SIZE: usize = 6;
    pub const BROADCAST: Self = Self([0xff; Self::ADDR_SIZE]);
    pub const NULL: Self = Self([0; Self::ADDR_SIZE]);
}

#[repr(u16)]
pub enum Type {
    Ip = 0x800u16.swap_bytes(),
    Arp = 0x0806u16.swap_bytes(),
}

#[repr(C, packed)]
pub struct Header {
    pub dest_mac: MacAddr,
    pub src_mac: MacAddr,
    pub typ: Type,
}

#[derive(Debug, Clone)]
pub struct Eth;

impl ConstPacketKind for Eth {
    const HSIZE: usize = core::mem::size_of::<Header>();
}

impl Packet<Eth> {
    pub fn create(typ: Type, mut size: usize) -> Packet<Eth> {
        size += Eth::HSIZE;

        let src_mac = super::default_device().mac();

        // Allocate the packet (needs to be 4KiB aligned).
        let layout = unsafe { Layout::from_size_align_unchecked(size, Size4KiB::SIZE as usize) };
        let ptr = DmaAllocator.allocate_zeroed(layout).expect("net: OOM!");
        let addr = VirtAddr::new(ptr.as_mut_ptr() as u64);

        let mut packet = Packet::<Eth>::new(addr, size);
        let header = packet.header_mut();

        header.src_mac = src_mac;
        header.typ = typ;

        packet
    }
}

impl PacketHeader<Header> for Packet<Eth> {
    fn send(&self) {
        let ip = match self.header().typ {
            Type::Ip => {
                let packet: Packet<ip::Ipv4> = self.upgrade();
                packet.header().dest_ip
            }

            Type::Arp => Ipv4Addr::BROADCAST,
        };

        if let Some(addr) = arp::get(ip) {
            let mut packet = self.clone();
            {
                let header = packet.header_mut();
                header.dest_mac = addr;
            }
            super::default_device().send(packet);
        } else {
            arp::request_ip(ip, self.clone());
        }
    }

    fn recv(&self) {
        match self.header().typ {
            Type::Ip => {
                let packet: Packet<ip::Ipv4> = self.upgrade();
                packet.recv()
            }

            Type::Arp => {
                let packet: Packet<arp::Arp> = self.upgrade();
                packet.recv()
            }
        }
    }
}
