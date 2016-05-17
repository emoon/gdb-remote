use std::net::TcpStream;
use std::io::{Read, Write};
use std::io;
use std::thread;

pub enum NeedsAck {
    Yes,
    No,
}

#[derive(Copy, Clone, PartialEq)]
pub enum State {
    Disconnected,
    Connected,
    SendingData,
    WaitingForData,
    Close,
}

pub struct GdbRemote {
    stream: Option<TcpStream>,
    temp_string: String,
    needs_ack: NeedsAck,
    packet_size: usize,
}

pub struct Memory {
    pub address: u64,
    pub data: Vec<u8>
}

impl Default for GdbRemote {
    fn default() -> Self {
        Self::new()
    }
}

const PACKET_SIZE: usize = 1024;
static HEX_CHARS: &'static [u8; 16] = b"0123456789abcdef";

#[inline]
fn calc_checksum(data: &[u8]) -> u8 {
    let mut checksum = 0u32;
    for i in data {
        checksum += *i as u32;
    }
    (checksum & 0xff) as u8
}

#[inline]
fn get_checksum(checksum: u8) -> (u8, u8) {
    (HEX_CHARS[((checksum >> 4) & 0xf) as usize],
     HEX_CHARS[(checksum & 0xf) as usize])
}

fn from_hex(ch: u8) -> u8 {
    if (ch >= b'a') && (ch <= b'f') {
        return ch - b'a' + 10;
    }

    if (ch >= b'0') && (ch <= b'9') {
        return ch - b'0';
    }

    if (ch >= b'A') && (ch <= b'F') {
        return ch - b'A' + 10;
    }

    0
}

impl GdbRemote {
    pub fn new() -> GdbRemote {
        GdbRemote {
            needs_ack: NeedsAck::Yes,
            temp_string: String::with_capacity(PACKET_SIZE + 4), // + 4 for header and checksum
            packet_size: PACKET_SIZE,
            stream: None,
        }
    }

    /*
    fn read_write_data(reader: &Receiver<Vec<u8>>, writer: &Sender<Vec<u8>>, stream: &mut TcpStream) -> io::Result<()> {
        match reader.recv() {
            Ok(data) => {
                // TODO: Less allocs
                let mut buffer = [0; 1024];
                let mut t = Vec::new();
                try!(stream.write_all(&data.into_boxed_slice()));
                let len = try!(stream.read(&mut buffer));
                for i in 0..len { t.push(buffer[i]); }
                match writer.send(t) {
                    Ok(()) => Ok(()),
                    Err(_) => Err(io::Error::new(io::ErrorKind::Other, "channel write failed")),
                }
            }

            Err(_) => Err(io::Error::new(io::ErrorKind::Other, "channel reade error")),
        }
    }
    */

    pub fn connect(&mut self, addr: &str) -> io::Result<()> {
        let stream = try!(TcpStream::connect(addr));
        self.stream = Some(stream);
        Ok(())
    }

    pub fn build_processed_string(dest: &mut String, source: &str) {
        let checksum = calc_checksum(source.as_bytes());
        let csum = get_checksum(checksum);

        dest.clear();
        dest.push('$');
        dest.push_str(source);
        dest.push('#');
        dest.push(csum.0 as char);
        dest.push(csum.1 as char);
    }

    pub fn send_command(&mut self, data: &str) -> io::Result<()> {
        Self::build_processed_string(&mut self.temp_string, data);
        self.send_command_raw()
    }

    pub fn send_command_raw(&mut self) -> io::Result<()> {
        if let Some(ref mut stream) = self.stream {
            stream.write_all(self.temp_string.as_bytes())
        } else {
            Ok(())
        }
    }

    //pub fn send_no_ack_request(&mut self)-> io::Result<usize> {
    //}

    /*
    pub fn get_data_sync(&mut self) -> Option<Vec<u8>> {
        match self.reader.recv() {
            Ok(data) => {
                self.temp_data.clear();
                for i in &data { self.temp_data.push(*i) }
                Some(self.temp_data.clone())
            }

            Err(_) => None,
        }
    }

    pub fn send_command_sync(&mut self, command: &str) -> Option<Vec<u8>> {
        if self.get_state() == State::Connected {
            self.send_command(command).unwrap();
            println!("waiting for data");
            let data = self.get_data_sync();
            println!("got data {}", data.as_ref().unwrap().len());
            data
        } else {
            None
        }
    }

    pub fn step_sync(&mut self) -> Option<Vec<u8>> {
        self.send_command_sync("s")
    }

    pub fn get_registers_sync(&mut self) -> Option<Vec<u8>> {
        self.send_command_sync("g")
    }
    */

    /*
    // TODO: 64-bit addresses
    pub fn get_memory_sync(&mut self, address: u32, size: u32) -> Option<Memory> {
        let mem_req = format!("m{:x},{:x}", address, size);

        if let Some(data) = self.send_command_sync(&mem_req) {
            let mut memory = Vec::with_capacity((data.len() - 2) / 2);
            let mut index = 1;

            for _ in 0..((data.len() - 2) / 2) {
                let v0 = data[index + 0];
                let v1 = data[index + 1];
                let v = (Self::from_hex(v0) << 4) | (Self::from_hex(v1));
                memory.push(v);
                index += 2;
            }

            Some(Memory {
                address: address as u64,
                data: memory,
            })
        } else {
            None
        }
    }
    */
}

#[test]
fn test_checksum_calc() {
    let data: [u8; 8] = [32, 32, 32, 32, 64, 64, 64, 64];
    assert_eq!(GdbRemote::calc_checksum(&data), 128);
}

#[test]
fn test_process_string() {
    assert_eq!(GdbRemote::get_processed_string("f"), "$f#66");
}

