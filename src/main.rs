use anyhow::{Context, Result};
use clap::Parser;
use std::net::Ipv4Addr;
use std::path::PathBuf;
use std::str::FromStr;
use tftpff::privilege;
use tftpff::server;
use tftpff::temp;

#[derive(Parser, Debug)]
#[clap(about, version, author)]
struct Args {
    #[clap(short, long)]
    dir: String,

    #[clap(short, long, default_value = "0.0.0.0")]
    addr: String,

    #[clap(short, long, default_value_t = 69)]
    port: u16,

    #[clap(short, long, default_value = "root")]
    user: String,

    #[clap(short, long, default_value = "root")]
    group: String,
}

fn main() -> Result<()> {
    env_logger::init();

    let args = Args::parse();

    let server_addr = Ipv4Addr::from_str(&args.addr)?;
    let server_port: u16 = args.port;
    let base_dir = PathBuf::from_str(&args.dir)?;

    let user: &str = &args.user;
    let group: &str = &args.group;

    let temp_dir = temp::create_temp_dir()?;
    privilege::chmod(temp_dir.path(), 0o777)?;
    privilege::chown(temp_dir.path(), user, group)?;

    let mut server = server::TftpServer::create(
        server_addr,
        server_port,
        base_dir,
        temp_dir.path().to_owned(),
    )
    .context("Failed to create TftpServer")?;
    server.bind().context("Failed to bind")?;
    privilege::drop_privilege(user, group)?;
    server.run().context("Failed in TftpServer running")?;

    return Ok(());
}
