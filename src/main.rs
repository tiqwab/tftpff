mod packet;

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
        let (n, client_addr) = sock.recv_from(&mut buf)?;
        if n == 0 {
            break;
        }

        let raw = &buf[..n];

        match packet::InitialPacket::parse(raw) {
            Ok(packet::InitialPacket::WRQ(wrq)) => {
                debug!("received: {:?}", wrq);
                let ack = packet::ACK::new(0);
                sock.send_to(&ack.encode(), client_addr)?;
                debug!("send: {:?}", ack);
            }
            Err(err) => {
                bail!("Failed to parse InitialPacket: {:?}", err);
            }
        }
    }

    return Ok(());
}
