mod packet;
mod temp_dir;

use anyhow::{bail, Context, Result};
use log::{debug, error};
use std::fs;
use std::io::Write;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::str::FromStr;

fn main() -> Result<()> {
    env_logger::init();

    // FIXME: Extract below as TftpServer struct
    let server_addr = Ipv4Addr::from_str("0.0.0.0")?;
    let server_port = 12345;
    let base_dir = Path::new("/tmp/tftpff");
    let temp_dir = temp_dir::create_temp_dir()?;

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
                debug!("received WRQ: {:?}", wrq);
                let mut block = 0;
                let ack = packet::ACK::new(block);
                sock.send_to(&ack.encode(), client_addr)?;
                block += 1;
                debug!("sent ack: {:?}", ack);

                let mut temp_file_path = PathBuf::from(temp_dir.path());
                temp_file_path.push(&wrq.filename);
                let mut temp_file = fs::File::create(&temp_file_path)?;
                debug!("created {:?}", temp_file_path);

                loop {
                    let (n, client_addr) = sock.recv_from(&mut buf)?;
                    let raw = &buf[..n];
                    match packet::Data::parse(raw) {
                        Ok(pkt) => {
                            debug!("received data: size={}", pkt.data().len());
                            if pkt.data().len() == 0 {
                                break;
                            }
                            temp_file.write(pkt.data())?;
                            let ack = packet::ACK::new(block);
                            sock.send_to(&ack.encode(), client_addr)?;
                            block += 1;
                            debug!("sent ack: {:?}", ack);

                            if pkt.data().len() < 512 {
                                break;
                            }
                        }
                        Err(err) => {
                            todo!()
                        }
                    }
                }

                let mut dest_path = PathBuf::from(base_dir);
                dest_path.push(&wrq.filename);
                fs::rename(temp_file_path, dest_path)?;
                debug!("finish WRQ for {:?}", wrq.filename)
            }
            Err(err) => {
                bail!("Failed to parse InitialPacket: {:?}", err);
            }
        }
    }

    return Ok(());
}
