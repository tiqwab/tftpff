mod packet;

use crate::packet::InitialPacket;
use anyhow::{bail, Context, Result};
use log::{debug, error};
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::str::FromStr;

fn main() -> Result<()> {
    env_logger::init();

    let server_addr = Ipv4Addr::from_str("0.0.0.0")?;
    let server_port = 12345;
    let sock_addr = SocketAddr::from((server_addr, server_port));
    let sock = UdpSocket::bind(sock_addr).context("Failed to bind")?;
    let mut buf = [0; 1024];

    debug!("listening at {}:{}", server_addr, server_port);

    loop {
        let (n, address) = sock.recv_from(&mut buf)?;
        if n == 0 {
            break;
        }

        let raw = &buf[..n];

        match InitialPacket::parse(raw) {
            Ok(InitialPacket::WRQ(wrq)) => {
                debug!("{:?}", wrq);
            }
            Err(err) => {
                error!("Failed to parse InitialPacket: {:?}", err);
            }
        }
    }

    return Ok(());
}
