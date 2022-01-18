use crate::packet;
use anyhow::Result;
use log::error;
use std::fmt::Formatter;
use std::io::ErrorKind;
use std::net::{SocketAddr, UdpSocket};
use std::{error, fmt, io};

#[derive(Debug)]
pub enum TftpError {
    Others,
    FileNotFound,
    AccessViolation,
    DiskNoSpace,
    IllegalTftpOp,
    UnknownTid,
    FileExists,
    NoSuchUser,
}

impl TftpError {
    pub fn from_u16(x: u16) -> Option<TftpError> {
        match x {
            0 => Some(TftpError::Others),
            1 => Some(TftpError::FileNotFound),
            2 => Some(TftpError::AccessViolation),
            3 => Some(TftpError::DiskNoSpace),
            4 => Some(TftpError::IllegalTftpOp),
            5 => Some(TftpError::UnknownTid),
            6 => Some(TftpError::FileExists),
            7 => Some(TftpError::NoSuchUser),
            _ => None,
        }
    }

    pub fn error_code(&self) -> u16 {
        match self {
            TftpError::Others => (0x0_u16),
            TftpError::FileNotFound => (0x1_u16),
            TftpError::AccessViolation => (0x2_u16),
            TftpError::DiskNoSpace => (0x3_u16),
            TftpError::IllegalTftpOp => (0x4_u16),
            TftpError::UnknownTid => (0x5_u16),
            TftpError::FileExists => (0x6_u16),
            TftpError::NoSuchUser => (0x7_u16),
        }
    }
}

impl fmt::Display for TftpError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            TftpError::Others => f.write_str("TftpError::Others"),
            TftpError::FileNotFound => f.write_str("TftpError::FileNotFound"),
            TftpError::AccessViolation => f.write_str("TftpError::AccessViolation"),
            TftpError::DiskNoSpace => f.write_str("TftpError::DiskNoSpace"),
            TftpError::IllegalTftpOp => f.write_str("TftpError::IllegalTftpOp"),
            TftpError::UnknownTid => f.write_str("TftpError::UnknownTid"),
            TftpError::FileExists => f.write_str("TftpError::FileExists"),
            TftpError::NoSuchUser => f.write_str("TftpError::NoSuchUser"),
        }
    }
}

impl error::Error for TftpError {}

pub trait TftpErrorNotifier<T, E> {
    fn notify_error(self, sock: &UdpSocket, client_addr: &SocketAddr) -> Result<T, E>;
}

impl<T> TftpErrorNotifier<T, io::Error> for Result<T, io::Error> {
    fn notify_error(self, sock: &UdpSocket, client_addr: &SocketAddr) -> Result<T, io::Error> {
        self.map_err(|err| match err.kind() {
            ErrorKind::NotFound => {
                send_error_packet(
                    sock,
                    client_addr,
                    TftpError::FileNotFound,
                    "File not found".to_string(),
                );
                err
            }
            ErrorKind::PermissionDenied => {
                send_error_packet(
                    sock,
                    client_addr,
                    TftpError::AccessViolation,
                    "Permission denied".to_string(),
                );
                err
            }
            _ => {
                send_error_packet(
                    sock,
                    client_addr,
                    TftpError::Others,
                    "Unexpected error".to_string(),
                );
                err
            }
        })
    }
}

fn send_error_packet(sock: &UdpSocket, client_addr: &SocketAddr, tftp_err: TftpError, msg: String) {
    let pkt = packet::Error::new(tftp_err, msg);
    match sock.send_to(&pkt.encode(), client_addr) {
        Ok(_) => (),
        Err(err) => error!(
            "Failed to send an error packet ({:?}), but ignore it: {:?}",
            pkt, err
        ),
    };
}
