use crate::packet;
use crate::packet::{ReadPacket, WritePacket};
use anyhow::{Context, Result};
use log::{debug, error, warn};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::thread::JoinHandle;

pub struct TftpServer {
    server_addr: Ipv4Addr,
    server_port: u16,
    base_dir: PathBuf,
    rrq_handler: Arc<Box<dyn Fn(UdpSocket, ReadPacket) -> () + Send + Sync>>,
    wrq_handler: Arc<Box<dyn Fn(UdpSocket, WritePacket) -> () + Send + Sync>>,
    server_sock: Option<UdpSocket>,
}

impl TftpServer {
    pub fn new(
        server_addr: Ipv4Addr,
        server_port: u16,
        base_dir: PathBuf,
        rrq_handler: Box<dyn Fn(UdpSocket, ReadPacket) -> () + Send + Sync>,
        wrq_handler: Box<dyn Fn(UdpSocket, WritePacket) -> () + Send + Sync>,
    ) -> TftpServer {
        TftpServer {
            server_addr,
            server_port,
            base_dir,
            rrq_handler: Arc::new(rrq_handler),
            wrq_handler: Arc::new(wrq_handler),
            server_sock: None,
        }
    }

    pub fn server_addr(&self) -> Option<SocketAddr> {
        self.server_sock
            .as_ref()
            .and_then(|sock| sock.local_addr().ok())
    }

    pub fn bind(&mut self) -> Result<()> {
        let server_sock_addr = SocketAddr::from((self.server_addr, self.server_port));
        let server_sock =
            UdpSocket::bind(server_sock_addr).context("Failed to bind server_sock")?;
        debug!("listening at {}:{}", self.server_addr, self.server_port);
        self.server_sock = Some(server_sock);
        return Ok(());
    }

    pub fn run(&self) -> Result<()> {
        let server_sock = self.server_sock.as_ref().unwrap();

        loop {
            let mut client_buf = [0; 1024];
            let (client_n, client_addr) = server_sock
                .recv_from(&mut client_buf)
                .context("Failed to receive packet")?;

            match packet::InitialPacket::parse(&client_buf[..client_n]) {
                Ok(packet::InitialPacket::WRQ(wrq)) => {
                    match UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) {
                        Ok(child_sock) => {
                            let h = self.spawn_wrq(child_sock, wrq);
                        }
                        Err(err) => {
                            error!("Failed to create child_sock for {:?}. {:?}", wrq, err);
                        }
                    }
                }
                Ok(packet::InitialPacket::RRQ(rrq)) => {
                    match UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) {
                        Ok(child_sock) => {
                            self.spawn_rrq(child_sock, rrq);
                        }
                        Err(err) => {
                            error!("Failed to create child_sock for {:?}. {:?}", rrq, err);
                        }
                    }
                }
                Err(err) => {
                    warn!("Ignore unknown packet (expected WRQ or RRQ): {:?}", err);
                }
            }
        }
    }

    fn spawn_rrq(&self, socket: UdpSocket, rrq: ReadPacket) -> JoinHandle<()> {
        let handler = Arc::clone(&self.rrq_handler);
        thread::spawn(move || (handler)(socket, rrq))
    }

    fn spawn_wrq(&self, socket: UdpSocket, wrq: WritePacket) -> JoinHandle<()> {
        let handler = Arc::clone(&self.wrq_handler);
        thread::spawn(move || (handler)(socket, wrq))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packet::Mode;
    use crate::temp_dir;
    use std::str::FromStr;
    use std::sync::Mutex;

    #[test]
    fn test_run() -> Result<()> {
        let temp_dir = temp_dir::create_temp_dir()?;
        let server_addr = Arc::new(Mutex::new(None));
        let rrq_queue = Arc::new(Mutex::new(vec![]));
        let wrq_queue = Arc::new(Mutex::new(vec![]));

        {
            let sa = Arc::clone(&server_addr);
            let rq = Arc::clone(&rrq_queue);
            let wq = Arc::clone(&wrq_queue);

            let rrq_handler = move |sock, pkt| {
                rq.lock().unwrap().push(pkt);
            };
            let wrq_handler = move |sock, pkt| {
                wq.lock().unwrap().push(pkt);
            };

            let mut server = TftpServer::new(
                Ipv4Addr::from_str("127.0.0.1")?,
                0,
                temp_dir.path().to_owned(),
                Box::new(rrq_handler),
                Box::new(wrq_handler),
            );

            let h = thread::spawn(move || {
                server.bind().unwrap();
                *sa.lock().unwrap() = Some(server.server_addr().unwrap());
                server.run().unwrap()
            });
        }

        thread::sleep(std::time::Duration::from_secs(1));
        println!("{:?}", server_addr.lock().unwrap());

        let server_addr = server_addr.lock().unwrap().unwrap();
        let sock_client = UdpSocket::bind(("127.0.0.1", 0))?;

        let rrq = ReadPacket::new("foo.txt".to_string(), Mode::OCTET);
        let wrq = WritePacket::new("bar.txt".to_string(), Mode::NETASCII);
        sock_client.send_to(&rrq.encode()[..], server_addr)?;
        sock_client.send_to(&wrq.encode()[..], server_addr)?;
        thread::sleep(std::time::Duration::from_secs(1));
        assert_eq!(rrq_queue.lock().unwrap().len(), 1);
        assert_eq!(wrq_queue.lock().unwrap().len(), 1);

        return Ok(());
    }
}
