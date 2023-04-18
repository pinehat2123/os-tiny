use simple_endian::BigEndian;

pub const PF_INET: u32 = 1;
pub const PF_INET6: u32 = 2;
pub const PF_UNIX: u32 = 3;
pub const PF_LOCAL: u32 = 3;
pub const PF_UNSPEC: u32 = 4;
pub const PF_NETLINK: u32 = 5;
pub const PF_BRIDGE: u32 = 6;

pub const AF_INET: u32 = PF_INET;
pub const AF_INET6: u32 = PF_INET6;
pub const AF_UNIX: u32 = PF_UNIX;
pub const AF_LOCAL: u32 = PF_LOCAL;
pub const AF_UNSPEC: u32 = PF_UNSPEC;
pub const AF_NETLINK: u32 = PF_NETLINK;
pub const AF_BRIDGE: u32 = PF_BRIDGE;

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum IpProtocol {
    Default = 0,
    Ip = 1,
    Ipv6 = 2,
    Icmp = 3,
    Raw = 4,
    Tcp = 5,
    Udp = 6,
    Igmp = 7,
    Ipip = 8,
    Dccp = 33,
    Routing = 43,
    Gre = 47,
    Esp = 50,
    Ah = 51,
    Icmpv6 = 58,
    Dstopts = 60,
    Comp = 108,
    Sctp = 132,
    Max = 256,
}

// mlibc/abi-bits/mlibc/socket.h
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum SocketType {
    Dgram = 1,
    Raw = 2,
    SeqPacket = 3,
    Stream = 4,
    Dccp = 5,
}

pub trait SocketAddr: Send + Sync {}

#[derive(Debug, Clone)]
#[repr(C)]
pub struct SocketAddrUnix {
    pub family: u32,
    pub path: [u8; 108],
}

impl Default for SocketAddrUnix {
    fn default() -> Self {
        Self {
            family: AF_UNIX,
            path: [0; 108],
        }
    }
}

#[derive(Debug, Clone)]
#[repr(C)]
pub struct InAddr {
    pub addr: u32,
}


#[derive(Debug, Clone)]
#[repr(C)]
pub struct SocketAddrInet {
    pub family: u32,
    pub port: BigEndian<u16>,
    pub sin_addr: InAddr,
    pub padding: [u8; 8],
}

impl SocketAddrInet {
    pub fn addr(&self) -> [u8; 4] {
        self.sin_addr.addr.to_le_bytes()
    }
    pub fn prot(&self) -> u16 {
        self.port.to_native()
    }
}

impl SocketAddr for SocketAddrUnix {}
impl SocketAddr for SocketAddrInet {}
