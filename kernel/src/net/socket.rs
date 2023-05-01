bitflags::bitflags! {
    pub struct MessageFlags: usize {
        const CTRUNC = 0x1;
        const DONTROUTE = 0x2;
        const EOR = 0x4;
        const OOB = 0x8;
        const NOSIGNAL = 0x10;
        const PEEK = 0x20;
        const TRUNC = 0x40;
        const WAITALL = 0x80;
        const FIN = 0x200;
        const CONFIRM = 0x800;

        // Linux extensions.
        const DONTWAIT = 0x1000;
        const CMSG_CLOEXEC = 0x2000;
        const MORE = 0x4000;
        const FASTOPEN = 0x20000000;
    }
}

pub mod tcp {
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use spin::Once;

use crate::fs::cache::DirCacheItem;
use crate::fs::file_table::FileHandle;
use crate::fs::inode::{FileType, INodeInterface, Metadata, PollFlags};
use crate::fs::{self, FileSystemError};
use crate::net::ip::Ipv4Addr;
use crate::net::tcp::{self, Tcp, TcpFlags, TcpHandler};
use crate::net::{Packet, PacketHeader, PacketTrait};
use crate::utils::sync::{Mutex, WaitQueue};

/// TCP Stream
struct Stream {
    buffer: Vec<u8>,
}

impl Stream {
    fn write(&mut self, buffer: &[u8]) {
        self.buffer.extend_from_slice(buffer);
    }

    fn read(&mut self, buffer: &mut [u8]) -> usize {
        let size = buffer.len().min(self.buffer.len());
        let target = self.buffer.drain(..size).collect::<Vec<_>>();

        buffer[..size].copy_from_slice(target.as_slice());
        size
    }

    fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }
}

impl Default for Stream {
    fn default() -> Self {
        Self { buffer: Vec::new() }
    }
}

#[derive(Default)]
struct TransmissionControl {
    /// Sequence number of the next byte to be sent.
    send_next: u32,
    recv_next: u32,
}

#[derive(Default, PartialEq, Eq, Debug)]
enum State {
    #[default]
    Closed,
    SynSent,
    Established,
}

#[derive(Default)]
struct TcpData {
    control: TransmissionControl,
    state: State,

    src_port: u16,
    dest_port: u16,
    target: Ipv4Addr,

    stream: Stream,
}

impl TcpData {
    fn make_packet(&self, size: usize, flags: TcpFlags) -> Packet<Tcp> {
        let mut packet = Packet::<Tcp>::create(self.src_port, self.dest_port, size, self.target);
        let header = packet.header_mut();

        header.set_sequence_number(self.control.send_next);
        header.set_window(u16::MAX);
        header.set_flags(flags);

        packet
    }

    fn make_ack_packet(&self, size: usize) -> Packet<Tcp> {
        let mut packet = self.make_packet(size, TcpFlags::empty());
        let header = packet.header_mut();

        header.set_ack_number(self.control.recv_next);
        packet
    }

    fn send_packet(&mut self, packet: Packet<Tcp>) {
        self.control.send_next = self.control.send_next.wrapping_add(packet.ack_len());
        packet.send();
    }

    fn send_sync(&mut self) {
        self.send_packet(self.make_packet(0, TcpFlags::SYN));
        self.state = State::SynSent;
    }

    fn recv(&mut self, packet: Packet<Tcp>) {
        let header = packet.header();

        match self.state {
            State::SynSent => {
                assert!(header.flags().contains(TcpFlags::ACK | TcpFlags::SYN));
                self.state = State::Established;
            }

            State::Established => {
                if !packet.as_slice().is_empty() {
                    let data = packet.as_slice();
                    self.stream.write(data);
                } else if header.flags().contains(TcpFlags::FIN) {
                    todo!()
                } else {
                    log::trace!("[ TCP ] Connection Established!");
                    return;
                }
            }

            State::Closed => unreachable!(),
        }

        self.control.recv_next = header.sequence_number().wrapping_add(packet.ack_len());
        self.send_packet(self.make_ack_packet(0));
    }
}

pub struct TcpSocket {
    sref: Weak<Self>,
    data: Mutex<TcpData>,
    handle: Once<Arc<FileHandle>>,
    wq: WaitQueue,
}

impl TcpSocket {
    const MAX_MTU: usize = 1460;

    pub fn new() -> Arc<Self> {
        Arc::new_cyclic(|sref| Self {
            handle: Once::new(),
            sref: sref.clone(),
            data: Mutex::new(TcpData::default()),
            wq: WaitQueue::new(),
        })
    }

    fn sref(&self) -> Arc<Self> {
        self.sref.upgrade().unwrap()
    }
}

impl INodeInterface for TcpSocket {
    fn metadata(&self) -> fs::Result<fs::inode::Metadata> {
        Ok(Metadata {
            id: 0,
            file_type: FileType::Socket,
            size: 0,
            children_len: 0,
        })
    }

    fn open(
        &self,
        _flags: aero_syscall::OpenFlags,
        handle: Arc<FileHandle>,
    ) -> fs::Result<Option<DirCacheItem>> {
        self.handle.call_once(|| handle);
        Ok(None)
    }

    fn bind(&self, _address: super::SocketAddr, _length: usize) -> fs::Result<()> {
        todo!()
    }

    fn connect(&self, address: super::SocketAddr, _length: usize) -> fs::Result<()> {
        let address = address.as_inet().ok_or(FileSystemError::NotSupported)?;
        let port = tcp::alloc_ephemeral_port(self.sref()).unwrap();

        let mut inner = self.data.lock_irq();
        inner.src_port = port;
        inner.dest_port = address.port();
        inner.target = Ipv4Addr::new(address.addr());

        inner.send_sync();
        Ok(())
    }

    fn read_at(&self, _offset: usize, buffer: &mut [u8]) -> fs::Result<usize> {
        let mut data = self
            .wq
            .block_on(&self.data, |e| e.state == State::Established)?;

        assert!(!data.stream.is_empty());
        Ok(data.stream.read(buffer))
    }

    fn recv(
        &self,
        message_hdr: &mut aero_syscall::socket::MessageHeader,
        _flags: aero_syscall::socket::MessageFlags,
    ) -> fs::Result<usize> {
        let mut data = self.data.lock_irq();
        assert!(!data.stream.is_empty());

        Ok(message_hdr
            .iovecs_mut()
            .iter_mut()
            .map(|iovec| {
                let iovec = iovec.as_slice_mut();
                data.stream.read(iovec)
            })
            .sum::<usize>())
    }

    fn write_at(&self, _offset: usize, buffer: &[u8]) -> fs::Result<usize> {
        let mut data = self
            .wq
            .block_on(&self.data, |e| e.state == State::Established)?;

        for chunk in buffer.chunks(Self::MAX_MTU) {
            let mut packet = data.make_ack_packet(chunk.len());
            packet.as_slice_mut().copy_from_slice(chunk);
            data.send_packet(packet);
        }

        Ok(buffer.len())
    }

    fn send(
        &self,
        message_hdr: &mut aero_syscall::socket::MessageHeader,
        _flags: aero_syscall::socket::MessageFlags,
    ) -> fs::Result<usize> {
        let data = message_hdr
            .iovecs()
            .iter()
            .map(|e| e.as_slice())
            .flatten()
            .copied()
            .collect::<Vec<_>>();

        let mut inner = self.data.lock_irq();

        for chunk in data.chunks(Self::MAX_MTU) {
            let mut packet = inner.make_ack_packet(chunk.len());
            packet.as_slice_mut().copy_from_slice(chunk);
            inner.send_packet(packet);
        }

        Ok(data.len())
    }

    fn poll(&self, _table: Option<&mut fs::inode::PollTable>) -> fs::Result<PollFlags> {
        let mut flags = PollFlags::empty();
        let data = self.data.lock_irq();

        if data.state == State::Closed {
            return Ok(flags);
        }

        flags |= PollFlags::OUT;

        if !data.stream.is_empty() {
            flags |= PollFlags::IN;
        }

        Ok(flags)
    }
}

impl TcpHandler for TcpSocket {
    fn recv(&self, packet: Packet<Tcp>) {
        self.data.lock_irq().recv(packet);
    }
}
}
pub mod udp {
use aero_syscall::prelude::{IfReq, SIOCGIFHWADDR, SIOCGIFINDEX, SIOCSIFADDR, SIOCSIFNETMASK};
use aero_syscall::socket::{MessageFlags, MessageHeader};
use aero_syscall::{OpenFlags, SocketAddrInet};
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use spin::Once;

use crate::fs::cache::DirCacheItem;
use crate::fs::file_table::FileHandle;
use crate::fs::inode::{FileType, INodeInterface, Metadata, PollFlags};
use crate::fs::{self, FileSystemError};
use crate::mem::paging::VirtAddr;
use crate::net::ip::Ipv4Addr;
use crate::net::udp::{self, Udp, UdpHandler};
use crate::net::{self, MacAddr, Packet, PacketHeader, PacketTrait};
use crate::utils::sync::{Mutex, WaitQueue};

use super::SocketAddr;

#[derive(Default)]
enum SocketState {
    /// The socket is not connected.
    #[default]
    Disconnected,
    Connected(SocketAddrInet),
}

#[derive(Default)]
struct UdpSocketInner {
    /// The address that the socket has been bound to.
    address: Option<SocketAddrInet>,
    state: SocketState,
    incoming: Vec<Packet<Udp>>,
}

pub struct UdpSocket {
    inner: Mutex<UdpSocketInner>,
    wq: WaitQueue,
    handle: Once<Arc<FileHandle>>,

    sref: Weak<Self>,
}

impl UdpSocket {
    pub fn new() -> Arc<Self> {
        Arc::new_cyclic(|sref| Self {
            wq: WaitQueue::new(),
            handle: Once::new(),

            inner: Mutex::new(Default::default()),
            sref: sref.clone(),
        })
    }

    fn sref(&self) -> Arc<Self> {
        self.sref.upgrade().unwrap()
    }

    fn set_state(&self, state: SocketState) {
        self.inner.lock_irq().state = state;
    }

    fn set_addr(&self, addr: SocketAddrInet) {
        self.inner.lock_irq().address = Some(addr);
    }

    fn src_port(&self) -> Option<u16> {
        self.inner
            .lock_irq()
            .address
            .as_ref()
            .map(|e| e.port.to_native())
    }

    fn dest(&self) -> SocketAddrInet {
        match &self.inner.lock_irq().state {
            SocketState::Connected(addr) => addr.clone(),
            _ => unreachable!(),
        }
    }

    pub fn is_non_block(&self) -> bool {
        self.handle
            .get()
            .expect("inet: not bound to an fd")
            .flags
            .read()
            .contains(OpenFlags::O_NONBLOCK)
    }
}

impl INodeInterface for UdpSocket {
    fn open(
        &self,
        _flags: aero_syscall::OpenFlags,
        handle: Arc<FileHandle>,
    ) -> fs::Result<Option<DirCacheItem>> {
        self.handle.call_once(|| handle);
        Ok(None)
    }

    fn metadata(&self) -> fs::Result<fs::inode::Metadata> {
        Ok(Metadata {
            id: 0,
            file_type: FileType::Socket,
            size: 0,
            children_len: 0,
        })
    }

    fn bind(&self, address: super::SocketAddr, _length: usize) -> fs::Result<()> {
        let address = address.as_inet().ok_or(FileSystemError::NotSupported)?;

        self.set_addr(address.clone());
        udp::bind(address.port.to_native(), self.sref());
        Ok(())
    }

    fn connect(&self, address: super::SocketAddr, _length: usize) -> fs::Result<()> {
        let address = address.as_inet().ok_or(FileSystemError::NotSupported)?;

        let host_addr = Ipv4Addr::new(address.sin_addr.addr.to_be_bytes());
        udp::connect(host_addr, address.port.to_native());

        self.set_state(SocketState::Connected(address.clone()));
        Ok(())
    }

    fn send(&self, message_hdr: &mut MessageHeader, _flags: MessageFlags) -> fs::Result<usize> {
        let name = message_hdr
            .name_mut::<SocketAddrInet>()
            .cloned()
            .unwrap_or_else(|| self.dest());

        let dest_port = name.port.to_native();
        let dest_ip = Ipv4Addr::new(name.addr());

        let src_port;

        if let Some(port) = self.src_port() {
            src_port = port;
        } else {
            src_port = udp::alloc_ephemeral_port(self.sref()).ok_or(FileSystemError::WouldBlock)?;
            log::debug!("Inet::send(): allocated ephemeral port {}", src_port);
        }

        // FIXME: loopback
        if dest_ip == Ipv4Addr::new([127, 0, 0, 1]) {
            return Err(FileSystemError::NotSupported);
        }

        let data = message_hdr
            .iovecs()
            .iter()
            .map(|e| e.as_slice())
            .flatten()
            .copied()
            .collect::<Vec<_>>();

        let mut packet = Packet::<Udp>::create(src_port, dest_port, data.len(), dest_ip);

        let dest = packet.as_slice_mut();
        dest.copy_from_slice(data.as_slice());

        packet.send();
        Ok(data.len())
    }

    fn recv(&self, message_hdr: &mut MessageHeader, _flags: MessageFlags) -> fs::Result<usize> {
        // assert!(flags.is_empty());

        if self.inner.lock_irq().incoming.is_empty() && self.is_non_block() {
            return Err(FileSystemError::WouldBlock);
        }

        let mut this = self.wq.block_on(&self.inner, |e| !e.incoming.is_empty())?;
        let packet = this.incoming.pop().expect("recv: someone was greedy");

        let mut data = packet.as_slice().to_vec();

        Ok(message_hdr
            .iovecs_mut()
            .iter_mut()
            .map(|iovec| {
                let iovec = iovec.as_slice_mut();
                let size = core::cmp::min(iovec.len(), data.len());
                iovec[..size].copy_from_slice(&data.drain(..size).collect::<Vec<_>>());
                size
            })
            .sum::<usize>())
    }

    fn ioctl(&self, command: usize, arg: usize) -> fs::Result<usize> {
        match command {
            SIOCGIFINDEX => {
                let ifreq = VirtAddr::new(arg as _).read_mut::<IfReq>()?;

                let name = ifreq.name().unwrap();
                assert!(name == "eth0");

                ifreq.data.ifindex = 1; // FIXME: Fill the actual interface index
                Ok(0)
            }

            SIOCGIFHWADDR => {
                let ifreq = VirtAddr::new(arg as _).read_mut::<IfReq>()?;

                let name = ifreq.name().ok_or(FileSystemError::InvalidPath)?;
                assert!(name == "eth0");

                let hwaddr = unsafe {
                    core::slice::from_raw_parts_mut(
                        ifreq.data.addr.sa_data.as_mut_ptr(),
                        MacAddr::ADDR_SIZE,
                    )
                };

                let mac_addr = net::default_device().mac();
                hwaddr.copy_from_slice(&mac_addr.0.as_slice());
                Ok(0)
            }

            SIOCSIFADDR => {
                let ifreq = VirtAddr::new(arg as _).read_mut::<IfReq>()?;
                let socket = SocketAddr::from_ifreq(ifreq)
                    .map_err(|_| FileSystemError::NotSupported)?
                    .as_inet()
                    .ok_or(FileSystemError::NotSupported)?;

                let name = ifreq.name().ok_or(FileSystemError::InvalidPath)?;

                // FIXME:
                assert!(name == "eth0");

                let device = net::default_device();
                device.set_ip(Ipv4Addr::new(socket.addr()));
                Ok(0)
            }

            SIOCSIFNETMASK => {
                let ifreq = VirtAddr::new(arg as _).read_mut::<IfReq>()?;
                let socket = SocketAddr::from_ifreq(ifreq)
                    .map_err(|_| FileSystemError::NotSupported)?
                    .as_inet()
                    .ok_or(FileSystemError::NotSupported)?;

                let name = ifreq.name().ok_or(FileSystemError::InvalidPath)?;

                // FIXME:
                assert!(name == "eth0");

                let device = net::default_device();
                device.set_subnet_mask(Ipv4Addr::new(socket.addr()));

                Ok(0)
            }

            _ => unreachable!("inet::ioctl(): unknown command {command}"),
        }
    }

    fn poll(&self, table: Option<&mut fs::inode::PollTable>) -> fs::Result<PollFlags> {
        if let Some(table) = table {
            table.insert(&self.wq);
        }

        let mut flags = PollFlags::OUT;

        if !self.inner.lock_irq().incoming.is_empty() {
            flags |= PollFlags::IN;
        }

        Ok(flags)
    }
}

impl UdpHandler for UdpSocket {
    fn recv(&self, packet: Packet<Udp>) {
        self.inner.lock_irq().incoming.push(packet);
        self.wq.notify_all();
    }
}
}
pub mod unix {
use aero_syscall::{OpenFlags, SocketAddrUnix, SyscallError, AF_UNIX};

use aero_syscall::socket::{MessageFlags, MessageHeader};

use alloc::collections::VecDeque;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use spin::Once;

use crate::fs;
use crate::fs::cache::DirCacheItem;
use crate::fs::file_table::FileHandle;
use crate::fs::inode::*;

use crate::fs::{FileSystemError, Path};

use crate::mem::paging::VirtAddr;
use crate::utils::sync::{Mutex, WaitQueue};

use super::SocketAddr;

fn path_from_unix_sock<'sock>(address: &'sock SocketAddrUnix) -> fs::Result<&'sock Path> {
    // The abstract namespace socket allows the creation of a socket
    // connection which does not require a path to be created.
    let abstract_namespaced = address.path[0] == 0;
    assert!(!abstract_namespaced);

    let path_len = address
        .path
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(address.path.len());

    let path_str = core::str::from_utf8(&address.path[..path_len])
        .ok()
        .ok_or(FileSystemError::InvalidPath)?;

    Ok(Path::new(path_str))
}

#[derive(Debug, Default)]
pub struct Message {
    data: Vec<u8>,
    // TODO: Keep track of the sender of the message here?
}

impl Message {
    pub fn new(data: Vec<u8>) -> Self {
        Self { data }
    }
}

#[derive(Default)]
pub struct MessageQueue {
    messages: VecDeque<Message>,
}

impl MessageQueue {
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    pub fn read(&mut self, buffer: &mut [u8]) -> usize {
        if let Some(message) = self.messages.front_mut() {
            let message_len = message.data.len();
            let size = core::cmp::min(buffer.len(), message_len);

            buffer[..size].copy_from_slice(&message.data[..size]);

            if size < message_len {
                message.data.drain(..size);
                return size;
            }

            self.messages.pop_front();
            size
        } else {
            unreachable!("MessageQueue::read() called when queue is empty");
        }
    }

    pub fn write(&mut self, buffer: &[u8]) {
        let message = Message::new(buffer.to_vec());
        self.messages.push_back(message);
    }
}

pub struct AcceptQueue {
    sockets: VecDeque<Arc<UnixSocket>>,
    backlog: usize,
}

impl AcceptQueue {
    /// # Parameters
    /// * `backlog`: The maximum number of pending connections that the queue can hold.
    pub fn new(backlog: usize) -> Self {
        Self {
            sockets: VecDeque::with_capacity(backlog),
            backlog,
        }
    }

    /// Returns `true` if the queue contains no pending connections.
    pub fn is_empty(&self) -> bool {
        self.sockets.is_empty()
    }

    /// Adds the given socket to the queue. Returns `EAGAIN` if the
    /// queue is full.
    pub fn push(&mut self, socket: Arc<UnixSocket>) -> Result<(), SyscallError> {
        if self.backlog == self.sockets.len() {
            return Err(SyscallError::EAGAIN);
        }

        self.sockets.push_back(socket);
        Ok(())
    }

    /// Removes the first pending connection from the queue and
    /// returns it, or [`None`] if it is empty.
    pub fn pop(&mut self) -> Option<Arc<UnixSocket>> {
        self.sockets.pop_front()
    }

    /// Updates the maximum number of pending connections that the
    /// queue can hold. Returns `EINVAL` if the new backlog is smaller
    /// than the current number of pending connections.
    pub fn set_backlog(&mut self, backlog: usize) -> Result<(), SyscallError> {
        if backlog < self.sockets.len() {
            return Err(SyscallError::EINVAL);
        }

        self.backlog = backlog;
        Ok(())
    }
}

#[derive(Default)]
enum UnixSocketState {
    /// The socket is not connected.
    #[default]
    Disconnected,

    /// The socket is listening for new connections.
    Listening(AcceptQueue),

    /// The socket has connected to a peer.
    Connected(Arc<UnixSocket>),
}

impl UnixSocketState {
    /// Returns `true` if the socket is connected.
    fn is_connected(&self) -> bool {
        matches!(self, Self::Connected(_))
    }

    fn queue(&mut self) -> Option<&mut AcceptQueue> {
        match self {
            Self::Listening(q) => Some(q),
            _ => None,
        }
    }
}

#[derive(Default)]
struct UnixSocketInner {
    /// The address that the socket has been bound to.
    address: Option<SocketAddrUnix>,

    state: UnixSocketState,
}

pub struct UnixSocket {
    inner: Mutex<UnixSocketInner>,
    buffer: Mutex<MessageQueue>,
    wq: WaitQueue,
    weak: Weak<UnixSocket>,
    handle: Once<Arc<FileHandle>>,
}

impl UnixSocket {
    pub fn new() -> Arc<Self> {
        Arc::new_cyclic(|weak| Self {
            inner: Mutex::new(UnixSocketInner::default()),

            buffer: Mutex::new(MessageQueue::default()),
            wq: WaitQueue::new(),
            weak: weak.clone(),
            handle: Once::new(),
        })
    }

    pub fn connect_pair(a: DirCacheItem, b: DirCacheItem) -> fs::Result<()> {
        let a = a
            .inode()
            .downcast_arc::<UnixSocket>()
            .ok_or(FileSystemError::NotSocket)?;

        let b = b
            .inode()
            .downcast_arc::<UnixSocket>()
            .ok_or(FileSystemError::NotSocket)?;

        a.inner.lock_irq().state = UnixSocketState::Connected(b.clone());
        b.inner.lock_irq().state = UnixSocketState::Connected(a.clone());
        Ok(())
    }

    pub fn sref(&self) -> Arc<Self> {
        self.weak.upgrade().unwrap()
    }

    pub fn is_non_block(&self) -> bool {
        self.handle
            .get()
            .expect("unix: not bound to an fd")
            .flags
            .read()
            .contains(OpenFlags::O_NONBLOCK)
    }
}

impl INodeInterface for UnixSocket {
    fn metadata(&self) -> fs::Result<Metadata> {
        Ok(Metadata {
            id: 0,
            file_type: FileType::Socket,
            size: 0,
            children_len: 0,
        })
    }

    fn open(
        &self,
        _flags: aero_syscall::OpenFlags,
        handle: Arc<FileHandle>,
    ) -> fs::Result<Option<DirCacheItem>> {
        self.handle.call_once(|| handle);
        Ok(None)
    }

    fn read_at(&self, _offset: usize, user_buffer: &mut [u8]) -> fs::Result<usize> {
        if self.buffer.lock_irq().is_empty() && self.is_non_block() {
            return Err(FileSystemError::WouldBlock);
        }

        let mut buffer = self.wq.block_on(&self.buffer, |e| !e.is_empty())?;

        let read = buffer.read(user_buffer);
        Ok(read)
    }

    fn write_at(&self, _offset: usize, buffer: &[u8]) -> fs::Result<usize> {
        let inner = self.inner.lock_irq();
        let peer = match inner.state {
            UnixSocketState::Connected(ref peer) => peer,
            _ => return Err(FileSystemError::NotConnected),
        };

        peer.buffer.lock_irq().write(buffer);
        peer.wq.notify_all();

        Ok(buffer.len())
    }

    fn listen(&self, backlog: usize) -> Result<(), SyscallError> {
        let mut inner = self.inner.lock_irq();
        let is_bound = inner.address.is_some();

        match &mut inner.state {
            // We cannot listen on a socket that has not been bound.
            UnixSocketState::Disconnected if is_bound => {
                inner.state = UnixSocketState::Listening(AcceptQueue::new(backlog));
                Ok(())
            }

            UnixSocketState::Listening(queue) => {
                queue.set_backlog(backlog)?;
                Ok(())
            }

            _ => unreachable!(),
        }
    }

    fn bind(&self, address: SocketAddr, _length: usize) -> fs::Result<()> {
        let address = address.as_unix().ok_or(FileSystemError::NotSupported)?;
        let path = path_from_unix_sock(address)?;

        if fs::lookup_path(path).is_ok() {
            return Err(FileSystemError::EntryExists);
        }

        let (parent, name) = path.parent_and_basename();
        DirEntry::from_socket_inode(fs::lookup_path(parent)?, String::from(name), self.sref())?;

        let mut inner = self.inner.lock_irq();
        inner.address = Some(address.clone());

        Ok(())
    }

    fn connect(&self, address: SocketAddr, _length: usize) -> fs::Result<()> {
        let address = address.as_unix().ok_or(FileSystemError::NotSupported)?;
        let path = path_from_unix_sock(address)?;
        let socket = fs::lookup_path(path)?;

        let target = socket
            .inode()
            .as_unix_socket()?
            .downcast_arc::<UnixSocket>()
            .ok_or(FileSystemError::NotSocket)?;

        let mut itarget = target.inner.lock_irq();

        let queue = match &mut itarget.state {
            UnixSocketState::Listening(queue) => queue,
            _ => return Err(FileSystemError::ConnectionRefused),
        };

        queue.push(self.sref()).unwrap();
        target.wq.notify_all();
        core::mem::drop(itarget); // release the lock

        let _ = self.wq.block_on(&self.inner, |e| e.state.is_connected())?;
        Ok(())
    }

    fn accept(&self, address: Option<(VirtAddr, &mut u32)>) -> fs::Result<Arc<UnixSocket>> {
        let mut inner = self.wq.block_on(&self.inner, |e| {
            e.state.queue().map(|x| !x.is_empty()).unwrap_or(false)
        })?;

        let queue = inner
            .state
            .queue()
            .ok_or(FileSystemError::ConnectionRefused)?;

        let peer = queue.pop().expect("UnixSocket::accept(): backlog is empty");
        let sock = Self::new();

        {
            let mut sock_inner = sock.inner.lock_irq();
            sock_inner.state = UnixSocketState::Connected(peer.clone());
        }

        {
            let mut peer_data = peer.inner.lock_irq();
            peer_data.state = UnixSocketState::Connected(sock.clone());
        }

        if let Some((address, length)) = address {
            let address = address.read_mut::<SocketAddrUnix>()?;

            if let Some(paddr) = peer.inner.lock_irq().address.as_ref() {
                *address = paddr.clone();
            } else {
                *address = SocketAddrUnix::default();
                address.family = AF_UNIX;
            }

            *length = core::mem::size_of::<SocketAddrUnix>() as u32;
        }

        peer.wq.notify_all();
        Ok(sock)
    }

    fn recv(&self, header: &mut MessageHeader, flags: MessageFlags) -> fs::Result<usize> {
        assert!(flags.is_empty());

        let inner = self.inner.lock_irq();

        let peer = match &inner.state {
            UnixSocketState::Connected(peer) => peer,
            _ => return Err(FileSystemError::NotConnected),
        };

        if self.buffer.lock_irq().is_empty() && self.is_non_block() {
            return Err(FileSystemError::WouldBlock);
        }

        let mut buffer = self.wq.block_on(&self.buffer, |e| !e.is_empty())?;

        header
            .name_mut::<SocketAddrUnix>()
            .map(|e| *e = peer.inner.lock_irq().address.as_ref().cloned().unwrap());

        Ok(header
            .iovecs_mut()
            .iter_mut()
            .map(|iovec| buffer.read(iovec.as_slice_mut()))
            .sum::<usize>())
    }

    fn poll(&self, table: Option<&mut PollTable>) -> fs::Result<PollFlags> {
        let buffer = self.buffer.lock_irq();
        let inner = self.inner.lock_irq();

        table.map(|e| e.insert(&self.wq));

        let mut events = PollFlags::OUT;

        match &inner.state {
            UnixSocketState::Listening(queue) => {
                if !queue.is_empty() {
                    events.insert(PollFlags::IN);
                    return Ok(events);
                }
            }

            _ => {}
        }

        if !buffer.is_empty() {
            events.insert(PollFlags::IN);
        }

        Ok(events)
    }
}
}
#[derive(Debug)]
#[repr(C)]
pub struct MessageHeader {
    /// Pointer to the socket address structure.
    name: *mut u8,
    /// Size of the socket address structure.
    name_len: usize,

    iovec: *mut IoVec, // todo: use Option<NonNull<IoVec>>
    iovec_len: i32,    // todo: use ffi::c_int

    control: *const u8,
    control_len: usize,

    flags: i32, // todo: use ffi::c_int
}

use crate::net::SocketAddr;
impl MessageHeader {
    pub fn name_mut<T: SocketAddr>(&mut self) -> Option<&mut T> {
        if self.name.is_null() {
            return None;
        }

        assert!(self.name_len == core::mem::size_of::<T>());

        // SAFETY: We know that the `name` pointer is valid and we have an exclusive reference to
        // it. The size of name is checked above with the size of `T` and `T` is a `SocketAddr` so,
        // its safe to create a mutable reference of `T` from the ptr.
        unsafe { Some(&mut *(self.name as *mut T)) }
    }

    pub fn iovecs(&self) -> &[IoVec] {
        // SAFETY: We know that the `iovec` pointer is valid, initialized.
        unsafe { core::slice::from_raw_parts(self.iovec, self.iovec_len as usize) }
    }

    pub fn iovecs_mut(&mut self) -> &mut [IoVec] {
        // SAFETY: We know that the `iovec` pointer is valid, initialized and we have exclusive
        // access so, its safe to construct a mutable slice from it.
        unsafe { core::slice::from_raw_parts_mut(self.iovec, self.iovec_len as usize) }
    }
}

// options/posix/include/bits/posix/iovec.h
#[derive(Debug)]
#[repr(C)]
pub struct IoVec {
    base: *mut u8, // todo: use Option<NonNull<u8>>
    len: usize,
}

impl IoVec {
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: We know that the `base` pointer is valid and initialized.
        unsafe { core::slice::from_raw_parts_mut(self.base, self.len) }
    }

    pub fn as_slice_mut(&mut self) -> &mut [u8] {
        // SAFETY: We know that the `base` pointer is valid, initialized and we have exclusive
        // access so, its safe to construct a mutable slice from it.
        unsafe { core::slice::from_raw_parts_mut(self.base, self.len) }
    }

    /// Returns the length of the I/O vector.
    pub fn len(&self) -> usize {
        self.len
    }
}
