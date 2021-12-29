use crate::packet::{ReadPacket, WritePacket};
use crate::{packet, temp_dir};
use anyhow::{bail, Context, Result};
use log::{debug, error, warn};
use std::io::{ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;
use std::{fs, thread, time};

pub struct TftpServer {
    server_addr: Ipv4Addr,
    server_port: u16,
    retry_interval: Duration,
    rrq_handler: Arc<Box<dyn Fn(UdpSocket, SocketAddr, ReadPacket) -> Result<()> + Send + Sync>>,
    wrq_handler: Arc<Box<dyn Fn(UdpSocket, SocketAddr, WritePacket) -> Result<()> + Send + Sync>>,
    server_sock: Option<UdpSocket>,
}

impl TftpServer {
    pub fn create(
        server_addr: Ipv4Addr,
        server_port: u16,
        base_dir: impl AsRef<Path>,
        temp_dir: impl AsRef<Path>,
    ) -> Result<TftpServer> {
        let rrq_handler = create_rrq_handler(base_dir.as_ref().to_owned());
        let wrq_handler =
            create_wrq_handler(base_dir.as_ref().to_owned(), temp_dir.as_ref().to_owned());
        Ok(TftpServer {
            server_addr,
            server_port,
            retry_interval: Duration::from_secs(5),
            rrq_handler: Arc::new(Box::new(rrq_handler)),
            wrq_handler: Arc::new(Box::new(wrq_handler)),
            server_sock: None,
        })
    }

    pub fn create_with_handlers(
        server_addr: Ipv4Addr,
        server_port: u16,
        rrq_handler: Box<dyn Fn(UdpSocket, SocketAddr, ReadPacket) -> Result<()> + Send + Sync>,
        wrq_handler: Box<dyn Fn(UdpSocket, SocketAddr, WritePacket) -> Result<()> + Send + Sync>,
    ) -> TftpServer {
        TftpServer {
            server_addr,
            server_port,
            retry_interval: Duration::from_secs(5),
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
                .context("Failed to receive request packet")?;

            match packet::InitialPacket::parse(&client_buf[..client_n]) {
                Ok(packet::InitialPacket::WRQ(wrq)) => {
                    match UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) {
                        Ok(child_sock) => {
                            child_sock.set_read_timeout(Some(self.retry_interval))?;
                            child_sock.set_write_timeout(Some(self.retry_interval))?;
                            self.spawn_wrq(child_sock, client_addr, wrq);
                        }
                        Err(err) => {
                            error!("Failed to create child_sock for {:?}. {:?}", wrq, err);
                        }
                    }
                }
                Ok(packet::InitialPacket::RRQ(rrq)) => {
                    match UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) {
                        Ok(child_sock) => {
                            child_sock.set_read_timeout(Some(self.retry_interval))?;
                            child_sock.set_write_timeout(Some(self.retry_interval))?;
                            self.spawn_rrq(child_sock, client_addr, rrq);
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

    fn spawn_rrq(
        &self,
        socket: UdpSocket,
        client_addr: SocketAddr,
        rrq: ReadPacket,
    ) -> JoinHandle<()> {
        let handler = Arc::clone(&self.rrq_handler);
        thread::spawn(move || {
            (handler)(socket, client_addr, rrq).unwrap_or_else(|err| {
                error!("Failed in handling RRQ from {}: {:?}", client_addr, err)
            })
        })
    }

    fn spawn_wrq(
        &self,
        socket: UdpSocket,
        client_addr: SocketAddr,
        wrq: WritePacket,
    ) -> JoinHandle<()> {
        let handler = Arc::clone(&self.wrq_handler);
        thread::spawn(move || {
            (handler)(socket, client_addr, wrq).unwrap_or_else(|err| {
                error!("Failed in handling WRQ from {}: {:?}", client_addr, err)
            })
        })
    }
}

pub fn create_rrq_handler(
    base_dir: PathBuf,
) -> impl Fn(UdpSocket, SocketAddr, ReadPacket) -> Result<()> {
    move |sock, client_addr, rrq| {
        debug!("received RRQ: {:?}", rrq);
        let mut block = 1;
        let mut buf = [0; 1024];

        let src_path = base_dir.join(&rrq.filename);
        let mut file =
            fs::File::open(&src_path).with_context(|| format!("Failed to open {:?}", src_path))?;

        loop {
            let mut file_buf = [0 as u8; 512];
            let file_n = file.read(&mut file_buf)?;
            let data_pkt = packet::Data::new(block, &file_buf[..file_n]);
            sock.send_to(&data_pkt.encode(), client_addr)?;

            let (ack_n, ack_addr) = sock.recv_from(&mut buf)?;
            if ack_addr != client_addr {
                warn!(
                    "receive packet from unknown client: {}. ignore it.",
                    ack_addr
                );
                // TODO: ignore
                continue;
            }

            match packet::ACK::parse(&buf[..ack_n]) {
                Ok(pkt) => {
                    if pkt.block() != block {
                        warn!("received ACK with wrong block.");
                        // TODO: resend
                    }
                }
                Err(err) => {
                    warn!("couldn't receive ACK: {:?}", err);
                    // TODO: resend
                }
            }
            block += 1;
            debug!("sent data: size={}", file_n);
            if file_n < 512 {
                break;
            }
        }

        debug!("finish RRQ for {:?}", rrq.filename);
        return Ok(());
    }
}

enum WrqHandlingState {
    RequestAccepted { trial_count: u16 },
    DataAccepted { block: u16, trial_count: u16 },
}

impl WrqHandlingState {
    const MAX_TRIAL_COUNT: u16 = 5;

    fn new() -> WrqHandlingState {
        WrqHandlingState::RequestAccepted { trial_count: 0 }
    }

    fn block(&self) -> u16 {
        match self {
            WrqHandlingState::RequestAccepted { .. } => 0,
            WrqHandlingState::DataAccepted { block, .. } => block.clone(),
        }
    }

    fn trial_count(&self) -> u16 {
        (match self {
            WrqHandlingState::RequestAccepted { trial_count } => trial_count,
            WrqHandlingState::DataAccepted { trial_count, .. } => trial_count,
        })
        .clone()
    }

    fn increment_trial_count(&mut self) -> Option<u16> {
        let cur = match self {
            WrqHandlingState::RequestAccepted { trial_count } => trial_count,
            WrqHandlingState::DataAccepted { trial_count, .. } => trial_count,
        };
        if *cur >= Self::MAX_TRIAL_COUNT {
            None
        } else {
            *cur += 1;
            Some(cur.clone())
        }
    }

    fn prepare_packet(&mut self) -> Option<packet::ACK> {
        self.increment_trial_count()
            .map(|_| packet::ACK::new(self.block()))
    }

    fn next(self) -> Self {
        match self {
            WrqHandlingState::RequestAccepted { .. } => WrqHandlingState::DataAccepted {
                block: 1,
                trial_count: 0,
            },
            WrqHandlingState::DataAccepted { block, .. } => WrqHandlingState::DataAccepted {
                block: block + 1,
                trial_count: 0,
            },
        }
    }
}

pub fn create_wrq_handler(
    base_dir: PathBuf,
    temp_dir: PathBuf,
) -> impl Fn(UdpSocket, SocketAddr, WritePacket) -> Result<()> {
    move |sock, client_addr, wrq| {
        debug!("[{}] received WRQ: {:?}", client_addr, wrq);
        let mut buf = [0; 1024];
        let mut state = WrqHandlingState::new();

        let ack = state.prepare_packet().unwrap();
        sock.send_to(&ack.encode(), client_addr)?;
        debug!("[{}] sent ack: {:?}", client_addr, ack);

        let temp_file_path = temp_dir.join(&wrq.filename);
        let mut temp_file = fs::File::create(&temp_file_path)?;
        debug!("[{}] created {:?}", client_addr, temp_file_path);

        loop {
            let (data_n, data_addr) = match sock.recv_from(&mut buf) {
                Ok(res) => res,
                Err(err) if err.kind() == ErrorKind::WouldBlock => {
                    // timeout
                    match state.prepare_packet() {
                        Some(pkt) => {
                            // retransmit
                            sock.send_to(&pkt.encode(), client_addr)?;
                            debug!(
                                "[{}] sent ack again (trial_count={}): {:?}",
                                client_addr,
                                state.trial_count(),
                                pkt
                            );
                            continue;
                        }
                        None => {
                            // exceed maximum retry count
                            bail!("Failed to receive data from {}: timeout", client_addr);
                        }
                    }
                }
                Err(err) => {
                    bail!("Failed to receive data from {}: {:?}", client_addr, err);
                }
            };

            if data_addr != client_addr {
                warn!(
                    "[{}] receive packet from unknown client: {}. ignore it.",
                    client_addr, data_addr
                );
                continue;
            }

            match packet::Data::parse(&buf[..data_n]) {
                Ok(pkt) => {
                    debug!("[{}] received data: size={}", client_addr, pkt.data().len());
                    temp_file.write(pkt.data())?;

                    state = state.next();
                    let ack = state.prepare_packet().unwrap();
                    sock.send_to(&ack.encode(), client_addr)?;
                    debug!("[{}] sent ack: {:?}", client_addr, ack);

                    if pkt.data().len() < 512 {
                        break;
                    }
                }
                Err(err) => {
                    warn!("[{}] receive unknown packet. ignore it.", client_addr,);
                }
            }
        }

        let dest_path = base_dir.join(&wrq.filename);
        fs::rename(temp_file_path, dest_path)?;
        debug!("[{}] finish WRQ for {:?}", client_addr, wrq.filename);
        return Ok(());
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

            let rrq_handler = move |sock, addr, pkt| {
                rq.lock().unwrap().push(pkt);
                Ok(())
            };
            let wrq_handler = move |sock, addr, pkt| {
                wq.lock().unwrap().push(pkt);
                Ok(())
            };

            let mut server = TftpServer::create_with_handlers(
                Ipv4Addr::from_str("127.0.0.1")?,
                0,
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
