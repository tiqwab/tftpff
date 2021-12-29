mod packet;

use anyhow::{bail, Context, Result};
use log::{debug, error, warn};
use std::net::Ipv4Addr;
use std::path::Path;
use std::str::FromStr;
use tftpff::server;
use tftpff::temp_dir;

fn main() -> Result<()> {
    env_logger::init();

    let server_addr = Ipv4Addr::from_str("0.0.0.0")?;
    let server_port = 12345;
    let base_dir = Path::new("/tmp/tftpff");
    let temp_dir = temp_dir::create_temp_dir()?;

    let mut server =
        server::TftpServer::create(server_addr, server_port, base_dir, temp_dir.path())
            .context("Failed to create TftpServer")?;
    server.bind().context("Failed to bind")?;
    server.run().context("Failed in TftpServer running")?;

    return Ok(());
}
