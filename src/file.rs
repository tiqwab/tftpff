use crate::packet;
use std::io::{Read, Write};
use std::path::Path;
use std::{fs, io};

/// This is a wrapper of std::fs::File.
/// The main purpose is parse and encode file content based on netascii if requested.
pub struct File {
    inner: fs::File,
    read_buf: Vec<u8>,
    write_buf: Vec<u8>,
    mode: packet::Mode,
    is_started: bool,
    is_finished: bool,
}

impl File {
    pub fn open(path: impl AsRef<Path>, mode: packet::Mode) -> io::Result<File> {
        let inner = fs::File::open(path)?;
        let read_buf = vec![];
        let write_buf = vec![];
        Ok(File {
            inner,
            read_buf,
            write_buf,
            mode,
            is_started: false,
            is_finished: false,
        })
    }

    pub fn create(path: impl AsRef<Path>, mode: packet::Mode) -> io::Result<File> {
        let inner = fs::File::create(path)?;
        let read_buf = vec![];
        let write_buf = vec![];
        Ok(File {
            inner,
            read_buf,
            write_buf,
            mode,
            is_started: false,
            is_finished: false,
        })
    }

    fn read_data_from_inner(&mut self) -> io::Result<usize> {
        let mut buf = [0; 512];
        let n_buf = self.inner.read(&mut buf)?;

        let initial_len = self.read_buf.len();

        for x in buf[..n_buf].iter() {
            if self.mode == packet::Mode::OCTET {
                self.read_buf.push(*x);
            } else {
                if *x == b'\r' {
                    self.read_buf.append(&mut vec![b'\r', b'\0']);
                } else if *x == b'\n' {
                    self.read_buf.append(&mut vec![b'\r', b'\n']);
                } else {
                    self.read_buf.push(*x);
                }
            }
        }

        Ok(self.read_buf.len() - initial_len)
    }

    pub fn has_next(&self) -> bool {
        // FIXME: this is just for read
        !self.is_started || !self.is_finished
    }
}

impl Read for File {
    fn read(&mut self, data: &mut [u8]) -> io::Result<usize> {
        if self.is_finished {
            return Ok(0);
        }

        if !self.is_started {
            self.is_started = true;
        }

        self.read_data_from_inner()?;

        let n = std::cmp::min(512, self.read_buf.len());
        // FIXME: is it efficient enough?
        for (i, x) in self.read_buf.drain(0..n).enumerate() {
            data[i] = x;
        }

        if n < 512 {
            self.is_finished = true;
        }

        Ok(n)
    }
}

impl Write for File {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.is_started = true;

        if self.mode == packet::Mode::OCTET {
            return self.inner.write(data);
        }

        let mut in_buf = vec![];
        let mut out_buf = vec![];

        // This is for the case when '\r' appears at the last byte in the previous data
        in_buf.append(&mut self.write_buf);
        in_buf.extend(data);

        let mut i = 0;
        while i < in_buf.len() {
            let cur_byte = in_buf[i];
            if cur_byte == b'\r' {
                i += 1;
                if i < in_buf.len() {
                    let following_byte = in_buf[i];
                    if following_byte == b'\0' {
                        out_buf.push(b'\r');
                    } else if following_byte == b'\n' {
                        out_buf.push(b'\n');
                    } else {
                        panic!(
                            "Failed to parse data: unexpected byte after '\\r': 0x{:x}",
                            following_byte
                        );
                    }
                } else {
                    self.write_buf.push(cur_byte);
                }
            } else {
                out_buf.push(cur_byte);
            }
            i += 1;
        }

        let n = self.inner.write(&out_buf)?;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        if !self.write_buf.is_empty() {
            self.inner.write(&self.write_buf)?;
        }
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::temp;

    fn do_test_read(content: &[u8], expected: &[u8], mode: packet::Mode) {
        //
        // setup
        //
        let temp_dir = temp::create_temp_dir().unwrap();
        let file_path = temp_dir.path().join("test_read.txt");
        let mut fs_file = fs::File::create(&file_path).unwrap();
        fs_file.write_all(&content);

        //
        // exercise
        //
        let mut my_file = File::open(&file_path, mode).unwrap();
        let mut my_buf = [0; 512];
        let my_n = my_file.read(&mut my_buf).unwrap();

        //
        // verify
        //
        assert_eq!(&my_buf[..my_n], expected);
    }

    #[test]
    fn test_read_with_netascii() {
        do_test_read(b"a\ra\na", b"a\r\0a\r\na", packet::Mode::NETASCII);
    }

    #[test]
    fn test_read_with_octet() {
        do_test_read(b"a\ra\na", b"a\ra\na", packet::Mode::OCTET);
    }

    #[test]
    fn test_read_512_multiple_bytes() {
        //
        // setup
        //
        let data = [vec![b'a'; 1022], vec![b'\n']].concat();

        let temp_dir = temp::create_temp_dir().unwrap();
        let file_path = temp_dir.path().join("test_read.txt");
        let mut fs_file = fs::File::create(&file_path).unwrap();
        fs_file.write_all(&data);

        //
        // exercise and verify
        //
        let mut my_file = File::open(&file_path, packet::Mode::NETASCII).unwrap();
        let mut my_buf = [0; 512];
        assert!(my_file.has_next());
        assert_eq!(my_file.read(&mut my_buf).unwrap(), 512);
        assert!(my_file.has_next());
        assert_eq!(my_file.read(&mut my_buf).unwrap(), 512);
        assert!(my_file.has_next());
        assert_eq!(my_file.read(&mut my_buf).unwrap(), 0);
        assert!(!my_file.has_next());
    }

    fn do_test_write(content: &[u8], expected: &[u8], mode: packet::Mode) {
        //
        // setup
        //
        let temp_dir = temp::create_temp_dir().unwrap();
        let file_path = temp_dir.path().join("test_write.txt");
        let mut my_file = File::create(&file_path, mode).unwrap();

        //
        // exercise
        //
        let my_buf = content;
        my_file.write(my_buf).unwrap();

        //
        // verify
        //
        let mut fs_file = fs::File::open(file_path).unwrap();
        let mut fs_buf = vec![];
        fs_file.read_to_end(&mut fs_buf);
        assert_eq!(&fs_buf, expected);
    }

    #[test]
    fn test_write_with_netascii() {
        do_test_write(b"a\r\0a\r\na", b"a\ra\na", packet::Mode::NETASCII);
    }

    #[test]
    fn test_write_with_octet() {
        do_test_write(b"a\r\0a\r\na", b"a\r\0a\r\na", packet::Mode::OCTET);
    }

    #[test]
    fn test_write_with_newlines() {
        //
        // setup
        //
        let temp_dir = temp::create_temp_dir().unwrap();
        let file_path = temp_dir.path().join("test_write.txt");
        let mut my_file = File::create(&file_path, packet::Mode::NETASCII).unwrap();

        //
        // exercise
        //
        let my_bufs = [
            [[b'a'; 511].to_vec(), [b'\r'].to_vec()].concat(),
            [[b'\0'].to_vec(), [b'a'; 510].to_vec(), [b'\r'].to_vec()].concat(),
            [b'\n'].to_vec(),
        ];
        for buf in my_bufs.iter() {
            my_file.write(buf).unwrap();
        }

        //
        // verify
        //
        let mut fs_file = fs::File::open(file_path).unwrap();
        let mut fs_buf = [0; 512];

        assert_eq!(fs_file.read(&mut fs_buf).unwrap(), 512);
        assert_eq!(fs_file.read(&mut fs_buf).unwrap(), 511);
    }
}
