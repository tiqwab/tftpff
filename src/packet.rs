use anyhow::{anyhow, bail, Result};
use std::fmt;
use std::fmt::Formatter;

#[derive(Debug, PartialEq, Eq)]
pub enum Mode {
    NETASCII,
    OCTET,
    MAIL,
}

impl Mode {
    pub fn parse(s: &[u8]) -> Option<Mode> {
        let s = String::from_utf8_lossy(s).to_ascii_lowercase();
        match s.as_str() {
            "netascii" => Some(Mode::NETASCII),
            "octet" => Some(Mode::OCTET),
            "mail" => Some(Mode::MAIL),
            _ => None,
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Mode::NETASCII => f.write_str("netascii"),
            Mode::OCTET => f.write_str("octet"),
            Mode::MAIL => f.write_str("mail"),
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

    fn parse(s: &[u8]) -> Result<WritePacket> {
        let opcode = u16::from_be_bytes(s[..2].try_into()?);
        if opcode != WritePacket::OPCODE {
            bail!("Illegal opcode as WRQ");
        }
        let s = &s[2..];
        let bs: Vec<&[u8]> = s.split(|x| *x == 0).collect();
        if bs.len() != 3 {
            bail!("Illegal packet as WRQ");
        }
        let filename = String::from_utf8_lossy(bs[0]).into_owned();
        let mode = Mode::parse(bs[1]).ok_or(anyhow!("Failed to parse mode"))?;
        Ok(WritePacket { filename, mode })
    }
}

#[derive(Debug)]
pub struct ReadPacket {
    pub filename: String,
    pub mode: Mode,
}

impl ReadPacket {
    const OPCODE: u16 = 0x01;

    fn parse(s: &[u8]) -> Result<ReadPacket> {
        let opcode = u16::from_be_bytes(s[..2].try_into()?);
        if opcode != ReadPacket::OPCODE {
            bail!("Illegal opcode as RRQ");
        }
        let s = &s[2..];
        let bs: Vec<&[u8]> = s.split(|x| *x == 0).collect();
        if bs.len() != 3 {
            bail!("Illegal packet as RRQ");
        }
        let filename = String::from_utf8_lossy(bs[0]).into_owned();
        let mode = Mode::parse(bs[1]).ok_or(anyhow!("Failed to parse mode"))?;
        Ok(ReadPacket { filename, mode })
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

    pub fn parse(s: &[u8]) -> Result<Data> {
        //  2 bytes     2 bytes      n bytes
        //  ----------------------------------
        // | Opcode |   Block #  |   Data     |
        //  ----------------------------------
        let opcode = u16::from_be_bytes(s[..2].try_into()?);
        if opcode != Data::OPCODE {
            bail!("Illegal opcode as Data: {}", opcode);
        }

        let block = u16::from_be_bytes(s[2..4].try_into()?);
        let data = s[4..].to_owned();

        Ok(Data { block, data })
    }

    pub fn encode(&self) -> Vec<u8> {
        let opcode = Data::OPCODE.to_be_bytes().to_vec();
        let block = self.block.to_be_bytes().to_vec();
        let data = self.data.clone();
        [opcode, block, data].concat()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_wrq_ok() -> Result<()> {
        // opcode=2, filename=Cargo.toml, mode=netascii
        let s = [
            0x00, 0x02, 0x43, 0x61, 0x72, 0x67, 0x6f, 0x2e, 0x74, 0x6f, 0x6d, 0x6c, 0x00, 0x6e,
            0x65, 0x74, 0x61, 0x73, 0x63, 0x69, 0x69, 0x00,
        ];
        let res = WritePacket::parse(&s)?;
        assert_eq!(res.filename, "Cargo.toml");
        assert_eq!(res.mode, Mode::NETASCII);
        return Ok(());
    }

    #[test]
    fn test_parse_wrq_with_illegal_mode() -> Result<()> {
        // opcode=2, filename=Cargo.toml, mode=n (illegal)
        let s = [
            0x00, 0x02, 0x43, 0x61, 0x72, 0x67, 0x6f, 0x2e, 0x74, 0x6f, 0x6d, 0x6c, 0x00, 0x6e,
            0x00,
        ];
        let res = WritePacket::parse(&s);
        assert!(res.is_err());
        return Ok(());
    }

    #[test]
    fn test_parse_rrq_ok() -> Result<()> {
        // opcode=1, filename=Cargo.toml, mode=netascii
        let s = [
            0x00, 0x01, 0x43, 0x61, 0x72, 0x67, 0x6f, 0x2e, 0x74, 0x6f, 0x6d, 0x6c, 0x00, 0x6e,
            0x65, 0x74, 0x61, 0x73, 0x63, 0x69, 0x69, 0x00,
        ];
        let res = ReadPacket::parse(&s)?;
        assert_eq!(res.filename, "Cargo.toml");
        assert_eq!(res.mode, Mode::NETASCII);
        return Ok(());
    }

    #[test]
    fn test_parse_rrq_with_illegal_mode() -> Result<()> {
        // opcode=1, filename=Cargo.toml, mode=n (illegal)
        let s = [
            0x00, 0x01, 0x43, 0x61, 0x72, 0x67, 0x6f, 0x2e, 0x74, 0x6f, 0x6d, 0x6c, 0x00, 0x6e,
            0x00,
        ];
        let res = ReadPacket::parse(&s);
        assert!(res.is_err());
        return Ok(());
    }

    #[test]
    fn test_parse_ack() -> Result<()> {
        let s = [0x00, 0x04, 0x00, 0x01];
        let ack = ACK::parse(&s)?;
        assert_eq!(ack.block(), 1);
        return Ok(());
    }

    #[test]
    fn test_encode_ack() -> Result<()> {
        let ack = ACK::new(1);
        assert_eq!(ack.encode(), vec![0x00, 0x04, 0x00, 0x01]);
        return Ok(());
    }

    #[test]
    fn test_parse_data() -> Result<()> {
        let s = [0x00, 0x03, 0x00, 0x01, 0x68, 0x65, 0x6c, 0x6c, 0x6f];
        let data = Data::parse(&s)?;
        assert_eq!(data.block(), 1);
        assert_eq!(data.data(), &s[4..]);
        return Ok(());
    }

    #[test]
    fn test_encode_data() -> Result<()> {
        let data = Data::new(1, &b"hello"[..]);
        assert_eq!(
            data.encode(),
            vec![0x00, 0x03, 0x00, 0x01, 0x68, 0x65, 0x6c, 0x6c, 0x6f]
        );
        return Ok(());
    }
}
