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
}

impl InitialPacket {
    pub fn parse(s: &[u8]) -> Result<InitialPacket> {
        let opcode = u16::from_be_bytes(s[..2].try_into()?);
        match opcode {
            1 => bail!("RRQ is not yet implemented"),
            2 => Ok(InitialPacket::WRQ(WritePacket::parse(s)?)),
            _ => bail!("Unknown InitialPacket"),
        }
    }
}

#[derive(Debug)]
pub struct WritePacket {
    filename: String,
    mode: Mode,
}

impl WritePacket {
    // FIXME: InitialPacket::WRQ cannot be type?
    fn parse(s: &[u8]) -> Result<WritePacket> {
        // skip opcode
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
}
