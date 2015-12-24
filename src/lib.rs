use std::net::TcpStream;
use std::net::SocketAddr;
use std::str::FromStr;
use std::str;
use std::io::Write;
use std::io;

pub struct GdbRemote {
    pub stream: TcpStream,
    pub needs_ack: bool,
}

static HEX_CHARS: &'static [u8; 16] = b"0123456789abcdef";

impl GdbRemote {
    pub fn connect(addr: &SocketAddr) -> Option<GdbRemote> {
        match TcpStream::connect(addr).map(|stream| stream) {
            Ok(s) => Some(GdbRemote { stream: s, needs_ack: true }),
            Err(_) => None,
        }
    }

    pub fn calc_checksum(data: &[u8]) -> u8 {
        let mut checksum = 0u32;
        for i in data {
            checksum += *i as u32;
        }
        (checksum & 0xff) as u8
    }

    #[inline]
    pub fn get_checksum_array(checksum: u8) -> [u8; 2] {
        let cs_string: [u8; 2] = [HEX_CHARS[((checksum >> 4) & 0xf) as usize],
                                  HEX_CHARS[(checksum & 0xf) as usize]];
        cs_string
    }

    pub fn calc_checksum_array(data: &[u8]) -> [u8; 2] {
        let checksum = Self::calc_checksum(data);
        Self::get_checksum_array(checksum)
    }

    pub fn get_checksum_string(checksum: u8) -> String {
        let cs_string = Self::get_checksum_array(checksum);
        unsafe {
            // Use unchecked here because we always know the range we use
            String::from_str(str::from_utf8_unchecked(&cs_string)).unwrap()
        }
    }

    pub fn get_processed_string(data: &str) -> String {
        let checksum = Self::calc_checksum(data.as_bytes());
        let cs_array = Self::get_checksum_array(checksum);

        // len + 4 = # <string> $xx (4 bytes extra needed) 
        let mut gdb_string = String::with_capacity(data.len() + 4);

        gdb_string.push('$');
        gdb_string.push_str(data);
        gdb_string.push('#');
        gdb_string.push(cs_array[0] as char);
        gdb_string.push(cs_array[1] as char);

        gdb_string
    }

    pub fn send_simple_command(&mut self, command: char) -> io::Result<usize> {
        // # command $ checksum
        let cs = Self::get_checksum_array(command as u8);
        let data: [u8; 5] = [b'$', command as u8, b'#', cs[0], cs[1]];
        self.stream.write(&data)
    }
}

#[test]
fn test_checksum_calc() {
    let data: [u8; 8] = [32, 32, 32, 32, 64, 64, 64, 64];
    assert_eq!(GdbRemote::calc_checksum(&data), 128);
}

#[test]
fn test_create_checksum_string() {
    assert_eq!(GdbRemote::get_checksum_string(0xf0), "f0");
    assert_eq!(GdbRemote::get_checksum_string(0xff), "ff");
    assert_eq!(GdbRemote::get_checksum_string(0x2f), "2f");
    assert_eq!(GdbRemote::get_checksum_string(0x87), "87");
    assert_eq!(GdbRemote::get_checksum_string(0xba), "ba");
}

#[test]
fn test_process_string() {
    assert_eq!(GdbRemote::get_processed_string("f"), "$f#66");
}

