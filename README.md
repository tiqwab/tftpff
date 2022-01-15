### Build

```
$ cargo build
```

### Run

Usage:

```
$ ./target/debug/tftpff --help
tftpff 0.0.1
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
$ sudo ./target/debug/tftpff --dir /tmp/tftpff --user root --group root
```

Run the server by non-privileged user with debug log:

```
$ sudo RUST_LOG=debug ./target/debug/tftpff --dir /tmp/tftpff --port 10069 --user nobody --group nobody
```
