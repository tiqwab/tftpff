use anyhow::Result;
use nix::sys::socket::{AddressFamily, InetAddr, SockAddr, SockFlag, SockType};
use std::net::{SocketAddr, UdpSocket};
use std::os::unix::io::{FromRawFd, RawFd};

/// Factory method for std::net::UdpSocket.
/// The inner socket has ReusePort and ReuseAddr options.
/// This is necessary because UdpSocket itself doesn't allow set options before bind.
pub fn create_udp_socket(addr: SocketAddr) -> Result<UdpSocket> {
    let fd = nix::sys::socket::socket(
        AddressFamily::Inet,
        SockType::Datagram,
        SockFlag::empty(),
        None,
    )?;
    reuse_port(fd)?;
    nix::sys::socket::bind(fd, &SockAddr::new_inet(InetAddr::from_std(&addr)))?;
    unsafe { Ok(UdpSocket::from_raw_fd(fd)) }
}

fn reuse_port(fd: RawFd) -> Result<()> {
    let opt = nix::sys::socket::sockopt::ReusePort;
    nix::sys::socket::setsockopt(fd, opt, &true)?;
    let opt = nix::sys::socket::sockopt::ReuseAddr;
    nix::sys::socket::setsockopt(fd, opt, &true)?;
    Ok(())
}
