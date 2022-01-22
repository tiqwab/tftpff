Firewall-friendly TFTP server.

As described in [#1](https://github.com/tiqwab/tftpff/pull/1), the server uses the same port for listening and responding to clients, which doesn't require additional firewall rules on client side.

### Build

```
$ cargo build --release
```

### Run

Usage:

```
$ ./target/release/tftpff --help
tftpff 0.1.0
Firewall-friendly tftp server

USAGE:
    tftpff [OPTIONS] --dir <DIR>

OPTIONS:
    -a, --addr <ADDR>      [default: 0.0.0.0]
    -d, --dir <DIR>
    -g, --group <GROUP>    [default: root]
    -h, --help             Print help information
    -p, --port <PORT>      [default: 69]
    -u, --user <USER>      [default: root]
    -V, --version          Print version information
```

Run the server with default port (69):

```
$ sudo ./target/release/tftpff --dir /tmp/tftpff --user root --group root
```

Run the server by non-privileged user with debug log:

```
$ sudo RUST_LOG=debug ./target/release/tftpff --dir /tmp/tftpff --port 10069 --user nobody --group nobody
```
