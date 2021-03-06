use crate::error::TftpErrorNotifier;
use crate::packet::{ReadPacket, WritePacket};
use crate::{file, packet, socket, temp};
use anyhow::{bail, Context, Result};
use log::{debug, error, warn};
use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;
use std::{fs, thread};

type RRQHandler = dyn Fn(UdpSocket, SocketAddr, ReadPacket) -> Result<()> + Send + Sync;
type WRQHandler = dyn Fn(UdpSocket, SocketAddr, WritePacket) -> Result<()> + Send + Sync;

pub struct TftpServer {
    server_addr: Ipv4Addr,
    server_port: u16,
    retry_interval: Duration,
    rrq_handler: Arc<RRQHandler>,
    wrq_handler: Arc<WRQHandler>,
    server_sock: Option<UdpSocket>,
}

impl TftpServer {
    pub fn create(
        server_addr: Ipv4Addr,
        server_port: u16,
        base_dir: impl AsRef<Path> + Send + Sync + 'static,
        temp_dir: impl AsRef<Path> + Send + Sync + 'static,
    ) -> Result<TftpServer> {
        let rrq_handler = create_rrq_handler(base_dir.as_ref().to_owned());
        let wrq_handler = create_wrq_handler(base_dir, temp_dir);
        Ok(TftpServer {
            server_addr,
            server_port,
            retry_interval: Duration::from_secs(5),
            rrq_handler: Arc::new(rrq_handler),
            wrq_handler: Arc::new(wrq_handler),
            server_sock: None,
        })
    }

    pub fn create_with_handlers(
        server_addr: Ipv4Addr,
        server_port: u16,
        rrq_handler: Box<RRQHandler>,
        wrq_handler: Box<WRQHandler>,
    ) -> TftpServer {
        TftpServer {
            server_addr,
            server_port,
            retry_interval: Duration::from_secs(5),
            rrq_handler: Arc::from(rrq_handler),
            wrq_handler: Arc::from(wrq_handler),
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
        let server_sock = socket::create_udp_socket(server_sock_addr)
            .context("Failed to create server socket")?;
        server_sock.set_read_timeout(Some(Duration::from_secs(1)))?;
        debug!("listening at {}:{}", self.server_addr, self.server_port);
        self.server_sock = Some(server_sock);
        Ok(())
    }

    pub fn run(&self) -> Result<()> {
        let server_sock = self.server_sock.as_ref().unwrap();
        let server_addr = server_sock.local_addr()?;

        // for graceful shutdown
        let term = Arc::new(AtomicBool::new(false));
        for &sig in signal_hook::consts::TERM_SIGNALS.iter() {
            signal_hook::flag::register(sig, Arc::clone(&term))?;
        }

        while !term.load(Ordering::Relaxed) {
            let mut client_buf = [0; 1024];
            let (client_n, client_addr) = match server_sock.recv_from(&mut client_buf) {
                Ok(res) => res,
                Err(err)
                    if [ErrorKind::WouldBlock, ErrorKind::Interrupted].contains(&err.kind()) =>
                {
                    continue;
                }
                Err(err) => {
                    bail!("Failed to receive request packet: {:?}", err);
                }
            };

            match packet::InitialPacket::parse(&client_buf[..client_n]) {
                Ok(packet::InitialPacket::WRQ(wrq)) => match socket::create_udp_socket(server_addr)
                {
                    Ok(child_sock) => {
                        child_sock.set_read_timeout(Some(self.retry_interval))?;
                        child_sock.set_write_timeout(Some(self.retry_interval))?;
                        child_sock.connect(&client_addr)?;
                        self.spawn_wrq(child_sock, client_addr, wrq);
                    }
                    Err(err) => {
                        error!("Failed to create child_sock for {:?}. {:?}", wrq, err);
                    }
                },
                Ok(packet::InitialPacket::RRQ(rrq)) => match socket::create_udp_socket(server_addr)
                {
                    Ok(child_sock) => {
                        child_sock.set_read_timeout(Some(self.retry_interval))?;
                        child_sock.set_write_timeout(Some(self.retry_interval))?;
                        child_sock.connect(&client_addr)?;
                        self.spawn_rrq(child_sock, client_addr, rrq);
                    }
                    Err(err) => {
                        error!("Failed to create child_sock for {:?}. {:?}", rrq, err);
                    }
                },
                Err(err) => {
                    warn!("Ignore unknown packet (expected WRQ or RRQ): {:?}", err);
                }
            }
        }

        Ok(())
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

struct RrqHandlingState {
    block: u16,
    trial_count: u16,
    data: Vec<u8>,
}

impl RrqHandlingState {
    const MAX_TRIAL_COUNT: u16 = 5;

    fn new() -> RrqHandlingState {
        RrqHandlingState {
            block: 0,
            trial_count: 0,
            data: vec![],
        }
    }

    fn block(&self) -> u16 {
        self.block
    }

    fn data(&self) -> &[u8] {
        self.data.as_slice()
    }

    fn trial_count(&self) -> u16 {
        self.trial_count
    }

    fn increment_trial_count(&mut self) -> Option<u16> {
        if self.trial_count() >= Self::MAX_TRIAL_COUNT {
            None
        } else {
            self.trial_count += 1;
            Some(self.trial_count())
        }
    }

    fn prepare_packet(&mut self) -> Option<packet::Data> {
        self.increment_trial_count()
            .map(|_| packet::Data::new(self.block(), self.data()))
    }

    fn next(&mut self, data: Vec<u8>) {
        self.block += 1;
        self.trial_count = 0;
        self.data = data;
    }
}

pub fn create_rrq_handler(
    base_dir: PathBuf,
) -> impl Fn(UdpSocket, SocketAddr, ReadPacket) -> Result<()> {
    move |sock, client_addr, rrq| {
        debug!("[{}] received RRQ: {:?}", client_addr, rrq);

        let src_path = base_dir.join(&rrq.filename);
        let mut file = file::File::open(&src_path, rrq.mode)
            .notify_error(&sock, &client_addr)
            .with_context(|| format!("Failed to open {:?}", src_path))?;
        let mut file_buf = [0_u8; 512];
        let mut file_n = file.read(&mut file_buf)?;

        let mut buf = [0; 1024];
        let mut state = RrqHandlingState::new();
        state.next(file_buf[..file_n].to_owned());

        let data = state.prepare_packet().unwrap();
        sock.send_to(&data.encode(), client_addr)?;
        debug!("[{}] sent data: {}", client_addr, data);

        loop {
            let (ack_n, ack_addr) = match sock.recv_from(&mut buf) {
                Ok(res) => res,
                Err(err) if err.kind() == ErrorKind::WouldBlock => {
                    // timeout
                    match state.prepare_packet() {
                        Some(pkt) => {
                            // retransmit
                            sock.send_to(&pkt.encode(), client_addr)?;
                            debug!(
                                "[{}] sent data again (trial_count={}): {}",
                                client_addr,
                                state.trial_count(),
                                pkt
                            );
                            continue;
                        }
                        None => {
                            // exceed maximum retry count
                            bail!("Failed to receive ack from {}: timeout", client_addr);
                        }
                    }
                }
                Err(err) => {
                    bail!("Failed to receive ack from {}: {:?}", client_addr, err);
                }
            };

            if ack_addr != client_addr {
                warn!(
                    "[{}] received packet from unknown client: {}. ignore it.",
                    client_addr, ack_addr
                );
                continue;
            }

            match packet::ACK::parse(&buf[..ack_n]) {
                Ok(pkt) if pkt.block() == state.block() => {
                    debug!("[{}] received ack: {:?}", client_addr, pkt);
                    if file.has_next() {
                        file_n = file.read(&mut file_buf)?;
                        state.next(file_buf[..file_n].to_owned());
                        match state.prepare_packet() {
                            Some(data) => {
                                sock.send_to(&data.encode(), client_addr)?;
                                debug!("[{}] sent data: {}", client_addr, data);
                            }
                            None => {
                                // shouldn't come here
                                continue;
                            }
                        }
                    } else {
                        break;
                    }
                }
                Ok(_pkt) => {
                    warn!("[{}] received ack with wrong block.", client_addr);
                }
                Err(err) => {
                    warn!(
                        "[{}] received unknown packet. ignore it: {:?}",
                        client_addr, err
                    );
                }
            }
        }

        debug!("[{}] finish RRQ for {:?}", client_addr, rrq.filename);
        Ok(())
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
            WrqHandlingState::DataAccepted { block, .. } => *block,
        }
    }

    fn trial_count(&self) -> u16 {
        *(match self {
            WrqHandlingState::RequestAccepted { trial_count } => trial_count,
            WrqHandlingState::DataAccepted { trial_count, .. } => trial_count,
        })
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
            Some(*cur)
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
    base_dir: impl AsRef<Path>,
    temp_dir: impl AsRef<Path>,
) -> impl Fn(UdpSocket, SocketAddr, WritePacket) -> Result<()> {
    move |sock, client_addr, wrq| {
        debug!("[{}] received WRQ: {:?}", client_addr, wrq);
        let mut buf = [0; 1024];
        let mut state = WrqHandlingState::new();

        let ack = state.prepare_packet().unwrap();
        sock.send_to(&ack.encode(), client_addr)?;
        debug!("[{}] sent ack: {:?}", client_addr, ack);

        let temp_file_path = temp_dir.as_ref().join(format!(
            "{}.{}",
            &wrq.filename,
            temp::generate_random_name()?
        ));
        let mut temp_file = file::File::create(&temp_file_path, wrq.mode)?;
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
                    "[{}] received packet from unknown client: {}. ignore it.",
                    client_addr, data_addr
                );
                continue;
            }

            match packet::Data::parse(&buf[..data_n]) {
                Ok(pkt) => {
                    debug!("[{}] received data: size={}", client_addr, pkt.data().len());
                    temp_file.write_all(pkt.data())?;

                    state = state.next();
                    let ack = state.prepare_packet().unwrap();
                    sock.send_to(&ack.encode(), client_addr)?;
                    debug!("[{}] sent ack: {:?}", client_addr, ack);

                    if pkt.data().len() < 512 {
                        break;
                    }
                }
                Err(err) => {
                    warn!(
                        "[{}] received unknown packet. ignore it: {:?}",
                        client_addr, err
                    );
                }
            }
        }

        let dest_path = base_dir.as_ref().join(&wrq.filename);
        // avoid using fs::rename (it cannot move if src and dest mount point are different)
        fs::copy(&temp_file_path, &dest_path)
            .notify_error(&sock, &client_addr)
            .with_context(|| format!("Failed to copy {:?} to {:?}", temp_file_path, dest_path))?;
        fs::remove_file(&temp_file_path)
            .with_context(|| format!("Failed to delete {:?}", temp_file_path))?;
        debug!("[{}] finish WRQ for {:?}", client_addr, wrq.filename);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::TftpError;
    use crate::packet::Mode;
    use crate::temp;
    use std::str::FromStr;
    use std::sync;
    use std::sync::Mutex;

    #[test]
    fn test_server_run() {
        let server_addr = Arc::new(Mutex::new(None));
        let rrq_queue = Arc::new(Mutex::new(vec![]));
        let wrq_queue = Arc::new(Mutex::new(vec![]));

        {
            let sa = Arc::clone(&server_addr);
            let rq = Arc::clone(&rrq_queue);
            let wq = Arc::clone(&wrq_queue);

            let rrq_handler = move |_sock, _addr, pkt| {
                rq.lock().unwrap().push(pkt);
                Ok(())
            };
            let wrq_handler = move |_sock, _addr, pkt| {
                wq.lock().unwrap().push(pkt);
                Ok(())
            };

            let mut server = TftpServer::create_with_handlers(
                Ipv4Addr::from_str("127.0.0.1").unwrap(),
                0,
                Box::new(rrq_handler),
                Box::new(wrq_handler),
            );

            let _h = thread::spawn(move || {
                server.bind().unwrap();
                *sa.lock().unwrap() = Some(server.server_addr().unwrap());
                server.run().unwrap()
            });
        }

        thread::sleep(std::time::Duration::from_secs(1));

        let server_addr = server_addr.lock().unwrap().unwrap();
        let sock_client = UdpSocket::bind(("127.0.0.1", 0)).unwrap();

        let rrq = ReadPacket::new("foo.txt".to_string(), Mode::OCTET);
        let wrq = WritePacket::new("bar.txt".to_string(), Mode::NETASCII);
        sock_client.send_to(&rrq.encode()[..], server_addr).unwrap();
        sock_client.send_to(&wrq.encode()[..], server_addr).unwrap();
        thread::sleep(std::time::Duration::from_secs(1));
        assert_eq!(rrq_queue.lock().unwrap().len(), 1);
        assert_eq!(wrq_queue.lock().unwrap().len(), 1);
    }

    #[test]
    fn test_rrq_handler() {
        //
        // setup
        //
        let base_dir = temp::create_temp_dir().unwrap();
        let handler = create_rrq_handler(base_dir.path().to_owned());

        let test_file_name = "test_wrq_handler.txt";
        let test_file_content = [b'a'; 513];
        {
            // prepare test file
            let mut test_file = fs::File::create(base_dir.path().join(test_file_name)).unwrap();
            test_file.write_all(&test_file_content).unwrap();
        }

        let sock_client = UdpSocket::bind(("127.0.0.1", 0)).unwrap();
        let addr_client = sock_client.local_addr().unwrap();
        let sock_handler = UdpSocket::bind(("127.0.0.1", 0)).unwrap();
        let addr_handler = sock_handler.local_addr().unwrap();
        sock_handler
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        sock_handler
            .set_write_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let rrq = packet::ReadPacket::new(test_file_name.to_string(), packet::Mode::OCTET);

        let _h = thread::spawn(move || {
            handler(sock_handler, addr_client, rrq).unwrap();
        });

        //
        // exercise and verify
        //
        let mut buf_client = [0; 1024];
        let mut actual_content: Vec<u8> = vec![];

        let (n_client, _) = sock_client.recv_from(&mut buf_client).unwrap();
        let data = packet::Data::parse(&buf_client[..n_client]).unwrap();
        assert_eq!(data.data().len(), 512);
        actual_content.append(&mut data.data().to_owned());
        sock_client
            .send_to(&packet::ACK::new(data.block()).encode(), addr_handler)
            .unwrap();

        let (n_client, _) = sock_client.recv_from(&mut buf_client).unwrap();
        let data = packet::Data::parse(&buf_client[..n_client]).unwrap();
        assert_eq!(data.data().len(), 1);
        actual_content.append(&mut data.data().to_owned());
        sock_client
            .send_to(&packet::ACK::new(data.block()).encode(), addr_handler)
            .unwrap();

        assert_eq!(&actual_content, &test_file_content);
    }

    #[test]
    fn test_rrq_handler_with_512_multiple_bytes() {
        env_logger::init();
        //
        // setup
        //
        let base_dir = temp::create_temp_dir().unwrap();
        let handler = create_rrq_handler(base_dir.path().to_owned());

        let test_file_name = "test_wrq_handler.txt";
        let test_file_content = [b'a'; 1024];
        {
            // prepare test file
            let mut test_file = fs::File::create(base_dir.path().join(test_file_name)).unwrap();
            test_file.write_all(&test_file_content).unwrap();
        }

        let sock_client = UdpSocket::bind(("127.0.0.1", 0)).unwrap();
        let addr_client = sock_client.local_addr().unwrap();
        let sock_handler = UdpSocket::bind(("127.0.0.1", 0)).unwrap();
        let addr_handler = sock_handler.local_addr().unwrap();
        sock_handler
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        sock_handler
            .set_write_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let rrq = packet::ReadPacket::new(test_file_name.to_string(), packet::Mode::OCTET);

        let _h = thread::spawn(move || {
            handler(sock_handler, addr_client, rrq).unwrap();
        });

        //
        // exercise and verify
        //
        let mut buf_client = [0; 1024];
        let mut actual_content: Vec<u8> = vec![];

        let (n_client, _) = sock_client.recv_from(&mut buf_client).unwrap();
        let data = packet::Data::parse(&buf_client[..n_client]).unwrap();
        assert_eq!(data.data().len(), 512);
        actual_content.append(&mut data.data().to_owned());
        sock_client
            .send_to(&packet::ACK::new(data.block()).encode(), addr_handler)
            .unwrap();

        let (n_client, _) = sock_client.recv_from(&mut buf_client).unwrap();
        let data = packet::Data::parse(&buf_client[..n_client]).unwrap();
        assert_eq!(data.data().len(), 512);
        actual_content.append(&mut data.data().to_owned());
        sock_client
            .send_to(&packet::ACK::new(data.block()).encode(), addr_handler)
            .unwrap();

        let (n_client, _) = sock_client.recv_from(&mut buf_client).unwrap();
        let data = packet::Data::parse(&buf_client[..n_client]).unwrap();
        assert_eq!(data.data().len(), 0);
        actual_content.append(&mut data.data().to_owned());
        sock_client
            .send_to(&packet::ACK::new(data.block()).encode(), addr_handler)
            .unwrap();

        assert_eq!(&actual_content, &test_file_content);
    }

    #[test]
    fn test_rrq_handler_with_error() {
        //
        // setup
        //
        let base_dir = temp::create_temp_dir().unwrap();
        let handler = create_rrq_handler(base_dir.path().to_owned());

        // this file doesn't exist, which should cause TftpError::FileNotFound
        let test_file_name = "test_wrq_handler.txt";

        let sock_client = UdpSocket::bind(("127.0.0.1", 0)).unwrap();
        let addr_client = sock_client.local_addr().unwrap();
        sock_client
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        sock_client
            .set_write_timeout(Some(Duration::from_secs(1)))
            .unwrap();

        let sock_handler = UdpSocket::bind(("127.0.0.1", 0)).unwrap();
        sock_handler
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        sock_handler
            .set_write_timeout(Some(Duration::from_secs(1)))
            .unwrap();

        let rrq = packet::ReadPacket::new(test_file_name.to_string(), packet::Mode::OCTET);

        let _h = thread::spawn(move || {
            handler(sock_handler, addr_client, rrq).unwrap();
        });

        //
        // exercise and verify
        //
        let mut buf_client = [0; 1024];

        let (n_client, _) = sock_client.recv_from(&mut buf_client).unwrap();
        let err_pkt = packet::Error::parse(&buf_client[..n_client]).unwrap();
        assert_eq!(err_pkt.error_code(), TftpError::FileNotFound.error_code());
        assert_eq!(err_pkt.message(), "File not found");
    }

    #[test]
    fn test_wrq_handler() {
        //
        // setup
        //
        let base_dir = temp::create_temp_dir().unwrap();
        let temp_dir = temp::create_temp_dir().unwrap();
        let test_file_name = "test_wrq_handler.txt";
        let handler = create_wrq_handler(base_dir.path().to_owned(), temp_dir.path().to_owned());

        let sock_client = UdpSocket::bind(("127.0.0.1", 0)).unwrap();
        let addr_client = sock_client.local_addr().unwrap();
        let sock_handler = UdpSocket::bind(("127.0.0.1", 0)).unwrap();
        let addr_handler = sock_handler.local_addr().unwrap();
        sock_handler
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        sock_handler
            .set_write_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        let wrq = packet::WritePacket::new(test_file_name.to_string(), packet::Mode::OCTET);

        let barrier_client = Arc::new(sync::Barrier::new(2));
        let barrier_handler = Arc::clone(&barrier_client);
        let _h = thread::spawn(move || {
            handler(sock_handler, addr_client, wrq).unwrap();
            barrier_handler.wait();
        });

        //
        // exercise and verify
        //
        let mut buf_client = [0; 1024];
        let content = [b'a'; 513];

        let (n_client, _) = sock_client.recv_from(&mut buf_client).unwrap();
        let ack = packet::ACK::parse(&buf_client[..n_client]).unwrap();
        assert_eq!(ack.block(), 0);

        let data = packet::Data::new(1, &content[..512]);
        sock_client.send_to(&data.encode(), addr_handler).unwrap();
        let (n_client, _) = sock_client.recv_from(&mut buf_client).unwrap();
        let ack = packet::ACK::parse(&buf_client[..n_client]).unwrap();
        assert_eq!(ack.block(), 1);

        let data = packet::Data::new(2, &content[512..]);
        sock_client.send_to(&data.encode(), addr_handler).unwrap();
        let (n_client, _) = sock_client.recv_from(&mut buf_client).unwrap();
        let ack = packet::ACK::parse(&buf_client[..n_client]).unwrap();
        assert_eq!(ack.block(), 2);

        barrier_client.wait();
        let mut file = fs::File::open(base_dir.path().join(test_file_name)).unwrap();
        let mut actual_content = vec![];
        file.read_to_end(&mut actual_content).unwrap();
        assert_eq!(&actual_content, &content);
    }

    #[test]
    fn test_wrq_handler_with_error() {
        //
        // setup
        //
        let base_dir = Path::new("/tmp/tftpff-unknown");

        let temp_dir = temp::create_temp_dir().unwrap();
        let test_file_name = "test_wrq_handler.txt";
        let handler = create_wrq_handler(base_dir.to_owned(), temp_dir.path().to_owned());

        let sock_client = UdpSocket::bind(("127.0.0.1", 0)).unwrap();
        let addr_client = sock_client.local_addr().unwrap();
        sock_client
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        sock_client
            .set_write_timeout(Some(Duration::from_secs(1)))
            .unwrap();

        let sock_handler = UdpSocket::bind(("127.0.0.1", 0)).unwrap();
        let addr_handler = sock_handler.local_addr().unwrap();
        sock_handler
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();
        sock_handler
            .set_write_timeout(Some(Duration::from_secs(1)))
            .unwrap();

        let wrq = packet::WritePacket::new(test_file_name.to_string(), packet::Mode::OCTET);

        let barrier_client = Arc::new(sync::Barrier::new(2));
        let barrier_handler = Arc::clone(&barrier_client);
        let _h = thread::spawn(move || {
            handler(sock_handler, addr_client, wrq).unwrap_or_else(|e| println!("{:?}", e));
            barrier_handler.wait();
        });

        //
        // exercise and verify
        //
        let mut buf_client = [0; 1024];
        let content = [b'a'; 513];

        let (n_client, _) = sock_client.recv_from(&mut buf_client).unwrap();
        let ack = packet::ACK::parse(&buf_client[..n_client]).unwrap();
        assert_eq!(ack.block(), 0);

        let data = packet::Data::new(1, &content[..512]);
        sock_client.send_to(&data.encode(), addr_handler).unwrap();
        let (n_client, _) = sock_client.recv_from(&mut buf_client).unwrap();
        let ack = packet::ACK::parse(&buf_client[..n_client]).unwrap();
        assert_eq!(ack.block(), 1);

        let data = packet::Data::new(2, &content[512..]);
        sock_client.send_to(&data.encode(), addr_handler).unwrap();
        let (n_client, _) = sock_client.recv_from(&mut buf_client).unwrap();
        let ack = packet::ACK::parse(&buf_client[..n_client]).unwrap();
        assert_eq!(ack.block(), 2);

        barrier_client.wait();

        let (n_client, _) = sock_client.recv_from(&mut buf_client).unwrap();
        let err = packet::Error::parse(&buf_client[..n_client]).unwrap();
        assert_eq!(err.error_code(), TftpError::FileNotFound.error_code());
        assert_eq!(err.message(), "File not found");
    }
}
