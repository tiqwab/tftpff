use crate::error::TftpError;
use anyhow::{anyhow, bail, Context, Result};
use std::fmt;
use std::fmt::Formatter;
use std::path::Path;

#[derive(Debug, PartialEq, Eq)]
pub enum Mode {
    NETASCII,
    OCTET,
}

impl Mode {
    pub fn parse(s: &[u8]) -> Option<Mode> {
        let s = String::from_utf8_lossy(s).to_ascii_lowercase();
        match s.as_str() {
            "netascii" => Some(Mode::NETASCII),
            "octet" => Some(Mode::OCTET),
            _ => None,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        match self {
            Mode::NETASCII => "netascii".as_bytes().to_vec(),
            Mode::OCTET => "octet".as_bytes().to_vec(),
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Mode::NETASCII => f.write_str("netascii"),
            Mode::OCTET => f.write_str("octet"),
        }
    }
}

#[derive(Debug)]
pub enum InitialPacket {
    WRQ(WritePacket),
    RRQ(ReadPacket),
}

impl InitialPacket {
    pub fn parse(s: &[u8]) -> Result<InitialPacket> {
        let opcode = u16::from_be_bytes(s[..2].try_into()?);
        match opcode {
            ReadPacket::OPCODE => Ok(InitialPacket::RRQ(ReadPacket::parse(s)?)),
            WritePacket::OPCODE => Ok(InitialPacket::WRQ(WritePacket::parse(s)?)),
            _ => bail!("Unknown InitialPacket"),
        }
    }
}

#[derive(Debug)]
pub struct WritePacket {
    pub filename: String,
    pub mode: Mode,
}

impl WritePacket {
    const OPCODE: u16 = 0x02;

    pub(crate) fn new(filename: String, mode: Mode) -> WritePacket {
        WritePacket { filename, mode }
    }

    fn parse(s: &[u8]) -> Result<WritePacket> {
        //  2 bytes     string    1 byte     string   1 byte
        //  ------------------------------------------------
        // | Opcode |  Filename  |   0  |    Mode    |   0  |
        //  ------------------------------------------------
        let opcode = u16::from_be_bytes(s[..2].try_into()?);
        if opcode != WritePacket::OPCODE {
            bail!("Illegal opcode as WRQ");
        }
        let s = &s[2..];
        let bs: Vec<&[u8]> = s.split(|x| *x == 0).collect();
        if bs.len() != 3 {
            bail!("Illegal packet as WRQ");
        }
        let raw_filename = String::from_utf8_lossy(bs[0]).into_owned();
        let filename = Path::new(&raw_filename)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .ok_or(anyhow!("Illegal format of filename: {}", raw_filename))?;
        let mode = Mode::parse(bs[1]).ok_or(anyhow!("Failed to parse mode"))?;
        Ok(WritePacket { filename, mode })
    }

    pub fn encode(&self) -> Vec<u8> {
        let opcode: Vec<u8> = WritePacket::OPCODE.to_be_bytes().to_vec();
        let filename: Vec<u8> = self.filename.as_bytes().to_vec();
        let mode: Vec<u8> = self.mode.encode();
        [opcode, filename, vec![0], mode, vec![0]].concat()
    }
}

#[derive(Debug)]
pub struct ReadPacket {
    pub filename: String,
    pub mode: Mode,
}

impl ReadPacket {
    const OPCODE: u16 = 0x01;

    pub(crate) fn new(filename: String, mode: Mode) -> ReadPacket {
        ReadPacket { filename, mode }
    }

    fn parse(s: &[u8]) -> Result<ReadPacket> {
        //  2 bytes     string    1 byte     string   1 byte
        //  ------------------------------------------------
        // | Opcode |  Filename  |   0  |    Mode    |   0  |
        //  ------------------------------------------------
        let opcode = u16::from_be_bytes(s[..2].try_into()?);
        if opcode != ReadPacket::OPCODE {
            bail!("Illegal opcode as RRQ");
        }
        let s = &s[2..];
        let bs: Vec<&[u8]> = s.split(|x| *x == 0).collect();
        if bs.len() != 3 {
            bail!("Illegal packet as RRQ");
        }
        let raw_filename = String::from_utf8_lossy(bs[0]).into_owned();
        let filename = Path::new(&raw_filename)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .ok_or(anyhow!("Illegal format of filename: {}", raw_filename))?;
        let mode = Mode::parse(bs[1]).ok_or(anyhow!("Failed to parse mode"))?;
        Ok(ReadPacket { filename, mode })
    }

    pub fn encode(&self) -> Vec<u8> {
        let opcode: Vec<u8> = ReadPacket::OPCODE.to_be_bytes().to_vec();
        let filename: Vec<u8> = self.filename.as_bytes().to_vec();
        let mode: Vec<u8> = self.mode.encode();
        [opcode, filename, vec![0], mode, vec![0]].concat()
    }
}

#[derive(Debug)]
pub struct ACK {
    block: u16,
}

impl ACK {
    const OPCODE: u16 = 0x04;

    pub fn new(block: u16) -> ACK {
        ACK { block }
    }

    pub fn block(&self) -> u16 {
        self.block
    }

    pub fn parse(s: &[u8]) -> Result<ACK> {
        //  2 bytes     2 bytes
        //  ---------------------
        // | Opcode |   Block #  |
        //  ---------------------
        let opcode = u16::from_be_bytes(s[..2].try_into()?);
        if opcode != ACK::OPCODE {
            bail!("Illegal opcode as Data: {}", opcode);
        }

        let block = u16::from_be_bytes(s[2..4].try_into()?);
        Ok(ACK { block })
    }

    pub fn encode(&self) -> Vec<u8> {
        let opcode: [u8; 2] = ACK::OPCODE.to_be_bytes();
        let block: [u8; 2] = self.block.to_be_bytes();
        [opcode, block].concat().into_iter().collect()
    }
}

pub struct Data {
    block: u16,
    data: Vec<u8>,
}

impl Data {
    const OPCODE: u16 = 0x03;

    pub fn new(block: u16, data: &[u8]) -> Data {
        Data {
            block,
            data: data.to_owned(),
        }
    }

    pub fn block(&self) -> u16 {
        self.block
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }

    pub fn parse(s: &[u8], mode: &Mode) -> Result<Data> {
        //  2 bytes     2 bytes      n bytes
        //  ----------------------------------
        // | Opcode |   Block #  |   Data     |
        //  ----------------------------------
        let opcode = u16::from_be_bytes(s[..2].try_into()?);
        if opcode != Data::OPCODE {
            bail!("Illegal opcode as Data: {}", opcode);
        }

        let block = u16::from_be_bytes(s[2..4].try_into()?);
        let data = if mode == &Mode::NETASCII {
            Self::parse_netascii(&s[4..])
        } else {
            s[4..].to_owned()
        };

        Ok(Data { block, data })
    }

    fn parse_netascii(data: &[u8]) -> Vec<u8> {
        let mut res = vec![];
        let mut i = 0;
        while i < data.len() {
            let x = data[i];
            if x == b'\r' {
                i += 1;
                let x = data[i];
                if x == b'\0' {
                    res.push(b'\r');
                } else if x == b'\n' {
                    res.push(b'\n');
                } else {
                    panic!(
                        "Failed to parse data: unexpected byte after '\r', 0x{:x}",
                        x
                    );
                }
            } else {
                res.push(x);
            }
            i += 1;
        }
        res
    }

    pub fn encode(&self, mode: &Mode) -> Vec<u8> {
        let opcode = Data::OPCODE.to_be_bytes().to_vec();
        let block = self.block.to_be_bytes().to_vec();
        let data = if mode == &Mode::NETASCII {
            Self::encode_netascii(&self.data)
        } else {
            self.data.clone()
        };
        [opcode, block, data].concat()
    }

    fn encode_netascii(data: &[u8]) -> Vec<u8> {
        let mut res = vec![];
        for x in data.iter() {
            if *x == b'\r' {
                res.append(&mut vec![b'\r', b'\0']);
            } else if *x == b'\n' {
                res.append(&mut vec![b'\r', b'\n']);
            } else {
                res.push(*x);
            }
        }
        res
    }
}

impl fmt::Display for Data {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_fmt(format_args!(
            "Data {{ block: {}, data: {} bytes }}",
            self.block,
            self.data.len()
        ))
    }
}

#[derive(Debug)]
pub struct Error {
    err: TftpError,
    msg: String,
}

impl Error {
    const OPCODE: u16 = 0x05;

    pub fn new(err: TftpError, msg: String) -> Error {
        Error { err, msg }
    }

    pub fn error_code(&self) -> u16 {
        self.err.error_code()
    }

    pub fn message(&self) -> &str {
        &self.msg
    }

    pub fn parse(data: &[u8]) -> Result<Error> {
        //  2 bytes     2 bytes      string    1 byte
        //  -----------------------------------------
        // | Opcode |  ErrorCode |   ErrMsg   |   0  |
        //  -----------------------------------------
        let opcode = u16::from_be_bytes(data[..2].try_into()?);
        if opcode != Error::OPCODE {
            bail!("Illegal opcode as Error");
        }

        let error_code = u16::from_be_bytes(data[2..4].try_into()?);
        let tftp_error = TftpError::from_u16(error_code).ok_or(anyhow!("Illegal error code"))?;

        if data.last() != Some(&b'\0') {
            bail!("Illegal packet as Error");
        }

        let msg = String::from_utf8_lossy(&data[4..(data.len() - 1)]).to_string();

        Ok(Error {
            err: tftp_error,
            msg,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let opcode = Error::OPCODE.to_be_bytes().to_vec();
        let error_code = self.err.error_code().to_be_bytes().to_vec();
        let msg = self.msg.as_bytes().to_vec();
        [opcode, error_code, msg, vec![b'\0']].concat()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_wrq_ok() {
        // opcode=2, filename=Cargo.toml, mode=netascii
        let s = [
            0x00, 0x02, 0x43, 0x61, 0x72, 0x67, 0x6f, 0x2e, 0x74, 0x6f, 0x6d, 0x6c, 0x00, 0x6e,
            0x65, 0x74, 0x61, 0x73, 0x63, 0x69, 0x69, 0x00,
        ];
        let res = WritePacket::parse(&s).unwrap();
        assert_eq!(res.filename, "Cargo.toml");
        assert_eq!(res.mode, Mode::NETASCII);
    }

    #[test]
    fn test_parse_wrq_with_illegal_mode() {
        // opcode=2, filename=Cargo.toml, mode=n (illegal)
        let s = [
            0x00, 0x02, 0x43, 0x61, 0x72, 0x67, 0x6f, 0x2e, 0x74, 0x6f, 0x6d, 0x6c, 0x00, 0x6e,
            0x00,
        ];
        let res = WritePacket::parse(&s);
        assert!(res.is_err());
    }

    #[test]
    fn test_parse_wrq_only_use_filename() {
        // opcode=2, filename=/foo/bar.txt, mode=netascii
        let s = [
            0x00, 0x02, 0x2f, 0x66, 0x6f, 0x6f, 0x2f, 0x62, 0x61, 0x72, 0x2e, 0x74, 0x78, 0x74,
            0x00, 0x6e, 0x65, 0x74, 0x61, 0x73, 0x63, 0x69, 0x69, 0x00,
        ];
        let res = WritePacket::parse(&s).unwrap();
        assert_eq!(res.filename, "bar.txt");
        assert_eq!(res.mode, Mode::NETASCII);
    }

    #[test]
    fn test_parse_rrq_ok() {
        // opcode=1, filename=Cargo.toml, mode=netascii
        let s = [
            0x00, 0x01, 0x43, 0x61, 0x72, 0x67, 0x6f, 0x2e, 0x74, 0x6f, 0x6d, 0x6c, 0x00, 0x6e,
            0x65, 0x74, 0x61, 0x73, 0x63, 0x69, 0x69, 0x00,
        ];
        let res = ReadPacket::parse(&s).unwrap();
        assert_eq!(res.filename, "Cargo.toml");
        assert_eq!(res.mode, Mode::NETASCII);
    }

    #[test]
    fn test_parse_rrq_with_illegal_mode() {
        // opcode=1, filename=Cargo.toml, mode=n (illegal)
        let s = [
            0x00, 0x01, 0x43, 0x61, 0x72, 0x67, 0x6f, 0x2e, 0x74, 0x6f, 0x6d, 0x6c, 0x00, 0x6e,
            0x00,
        ];
        let res = ReadPacket::parse(&s);
        assert!(res.is_err());
    }

    #[test]
    fn test_parse_rrq_only_use_filename() {
        // opcode=1, filename=/foo/bar.txt, mode=netascii
        let s = [
            0x00, 0x01, 0x2f, 0x66, 0x6f, 0x6f, 0x2f, 0x62, 0x61, 0x72, 0x2e, 0x74, 0x78, 0x74,
            0x00, 0x6e, 0x65, 0x74, 0x61, 0x73, 0x63, 0x69, 0x69, 0x00,
        ];
        let res = ReadPacket::parse(&s).unwrap();
        assert_eq!(res.filename, "bar.txt");
        assert_eq!(res.mode, Mode::NETASCII);
    }

    #[test]
    fn test_parse_ack() {
        let s = [0x00, 0x04, 0x00, 0x01];
        let ack = ACK::parse(&s).unwrap();
        assert_eq!(ack.block(), 1);
    }

    #[test]
    fn test_encode_ack() {
        let ack = ACK::new(1);
        assert_eq!(ack.encode(), vec![0x00, 0x04, 0x00, 0x01]);
    }

    #[test]
    fn test_parse_data() {
        let s = [0x00, 0x03, 0x00, 0x01, 0x68, 0x65, 0x6c, 0x6c, 0x6f];
        let mode = Mode::OCTET;
        let data = Data::parse(&s, &mode).unwrap();
        assert_eq!(data.block(), 1);
        assert_eq!(data.data(), &s[4..]);
    }

    #[test]
    fn test_parse_data_netascii() {
        let s = [
            vec![0x00, 0x03, 0x00, 0x01],
            vec![b'a', b'\r', b'\0', b'a', b'\r', b'\n', b'a'],
        ]
        .concat();
        let mode = Mode::NETASCII;
        let data = Data::parse(&s, &mode).unwrap();
        assert_eq!(data.block(), 1);
        assert_eq!(data.data(), &vec![b'a', b'\r', b'a', b'\n', b'a']);
    }

    #[test]
    fn test_encode_data() {
        let data = Data::new(1, &b"hello"[..]);
        let mode = Mode::OCTET;
        assert_eq!(
            data.encode(&mode),
            vec![0x00, 0x03, 0x00, 0x01, 0x68, 0x65, 0x6c, 0x6c, 0x6f]
        );
    }

    #[test]
    fn test_encode_data_netascii() {
        let data = Data::new(1, &vec![b'a', b'\r', b'a', b'\n', b'a']);
        let mode = Mode::NETASCII;
        assert_eq!(
            data.encode(&mode),
            [
                vec![0x00, 0x03, 0x00, 0x01],
                vec![b'a', b'\r', b'\0', b'a', b'\r', b'\n', b'a']
            ]
            .concat(),
        );
    }

    #[test]
    fn test_parse_error() {
        let data = [
            vec![0x00, 0x05, 0x00, 0x01],
            "File not found".to_string().as_bytes().to_vec(),
            vec![b'\0'],
        ]
        .concat();

        let pkt = Error::parse(&data).unwrap();
        assert_eq!(pkt.error_code(), 1);
        assert_eq!(pkt.message(), "File not found");
    }

    #[test]
    fn test_encode_error() {
        let err = Error::new(TftpError::FileNotFound, "File not found".to_string());
        assert_eq!(
            err.encode(),
            [
                vec![0x00, 0x05, 0x00, 0x01],
                "File not found".to_string().as_bytes().to_vec(),
                vec![b'\0'],
            ]
            .concat(),
        );
    }
}
