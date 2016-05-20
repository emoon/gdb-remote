use std::net::{TcpStream, ToSocketAddrs};
use std::io::{Read, Write};
use std::io;
use std::time::Duration;

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
    hex_to_byte: [u8; 256],
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

fn build_hex_to_byte_table() -> [u8; 256] {
    let mut table = [0; 256];
    for (i, item) in table.iter_mut().enumerate().take(256) {
        *item = from_hex(i as u8)
    }
    table
}

fn from_pair_hex(data: (u8, u8)) -> u8 {
    let t0 = from_hex(data.0);
    let t1 = from_hex(data.1);
    (t0 << 4) | t1
}

impl GdbRemote {
    pub fn new() -> GdbRemote {
        GdbRemote {
            needs_ack: NeedsAck::Yes,
            temp_string: String::with_capacity(PACKET_SIZE + 4), // + 4 for header and checksum
            stream: None,
            hex_to_byte: build_hex_to_byte_table(),
        }
    }

    pub fn connect<A: ToSocketAddrs>(&mut self, addr: A) -> io::Result<()> {
        let stream = try!(TcpStream::connect(addr));
        // 2 sec of time-out to make sure we never gets infitite blocked
        try!(stream.set_read_timeout(Some(Duration::from_secs(2))));
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

    pub fn send_internal(&mut self) -> io::Result<()> {
        if let Some(ref mut stream) = self.stream {
            stream.write_all(self.temp_string.as_bytes())
        } else {
            Err(io::Error::new(io::ErrorKind::Other, "No connection with a server."))
        }
    }

    pub fn send_command(&mut self, data: &str) -> io::Result<()> {
        Self::build_processed_string(&mut self.temp_string, data);
        self.send_internal()
    }

    fn handle_send_ack(stream: &mut TcpStream, resend_data: &str, dest: &mut [u8]) -> io::Result<usize> {
        loop {
            let mut v = [0; 1];
            try!(stream.read(&mut v));

            match v[0] {
                b'+' => return stream.read(dest),
                b'-' => try!(stream.write_all(resend_data.as_bytes())),
                _ => {
                    return Err(io::Error::new(io::ErrorKind::Other, "Illegal reply from server."))
                }
            }
        }
    }

    pub fn read_reply(&mut self, dest: &mut [u8]) -> io::Result<usize> {
        if let Some(ref mut stream) = self.stream {
            match self.needs_ack {
                NeedsAck::Yes => Self::handle_send_ack(stream, &self.temp_string.clone(), dest),
                NeedsAck::No => stream.read(dest),
            }
        } else {
            Err(io::Error::new(io::ErrorKind::Other, "No connection with a server."))
        }
    }

    fn clone_slice(dst: &mut [u8], src: &[u8]) {
        for (d, s) in dst.iter_mut().zip(src.iter()) {
            *d = *s;
        }
    }

    fn validate_checksum(data: &[u8], len: usize) -> io::Result<usize> {
        // - 3 to skip $xx in the end
        let data_len = len;
        let checksum = calc_checksum(&data[1..len - 3]);
        let data_check = from_pair_hex((data[data_len - 2], data[data_len - 1]));

        let data_string_got = String::from_utf8_lossy(&data[0..len]).into_owned();

        println!("Data got {}", data_string_got);

        if data_check == checksum {
            Ok((data_len))
        } else {
            println!("missmatch checksum {} - {}", data_check, checksum);
            Err(io::Error::new(io::ErrorKind::Other, "Checksum missmatch for data"))
        }
    }

    #[no_mangle]
    pub fn convert_hex_data_to_binary(&self, dest: &mut [u8], src: &[u8]) {
        for (d, s) in dest.iter_mut().zip(src.chunks(2)) {
            let v0 = s[0] as usize;
            let v1 = s[1] as usize;
            *d = (self.hex_to_byte[v0] << 4) | self.hex_to_byte[v1];
        }
    }

    pub fn send_command_wait_reply_raw(&mut self, res: &mut [u8], command: &str) -> io::Result<usize> {
        let mut temp_buffer = [0; PACKET_SIZE];
        try!(self.send_command(command));
        let len = try!(self.read_reply(&mut temp_buffer));
        try!(Self::validate_checksum(&temp_buffer, len));
        Self::clone_slice(res, &temp_buffer[1..len - 3]);
        Ok((len - 4))
    }

    pub fn get_supported(&mut self, res: &mut [u8]) -> io::Result<usize> {
        self.send_command_wait_reply_raw(res, "qSupported")
    }

    pub fn step_sync(&mut self, res: &mut [u8]) -> io::Result<usize> {
        self.send_command_wait_reply_raw(res, "s")
    }

    pub fn get_registers(&mut self, res: &mut [u8]) -> io::Result<usize> {
        let mut temp_buffer = [0; PACKET_SIZE];
        let len = try!(self.send_command_wait_reply_raw(&mut temp_buffer, "g"));
        self.convert_hex_data_to_binary(res, &temp_buffer[0..len]);
        Ok((len / 2))
    }

    pub fn get_memory(&mut self, dest: &mut Vec<u8>, address: u64, size: u64) -> io::Result<usize> {
        let mut temp_buffer = [0; PACKET_SIZE];
        let mut temp_unpack = [0; PACKET_SIZE/2];

        if size == 0 {
            return Ok((0));
        }

        dest.clear();

        // Text hexdata and 4 bytes of header bytes gives this max amount of requested data / loop
        let size_per_loop = ((PACKET_SIZE - 4) / 2) as u64;
        let mut data_size = size;
        let mut addr = address;
        let mut trans_size = 0u64;

        loop {
            let current_size = if data_size < size_per_loop { data_size } else { size_per_loop };

            // TODO: This allocates memory, would be nice to format to existing string.

            let mem_req = format!("m{:x},{:x}", addr, current_size);

            let len = try!(self.send_command_wait_reply_raw(&mut temp_buffer, &mem_req));

            if temp_buffer[0] == b'E' {
                // Ok, something went wrong here. If we have already got some data we just
                // return what we got (it might be the case that some of the data was ok
                // to read but the other data wasn't (un-mapped memory for example)
                if trans_size == 0 {
                    return Err(io::Error::new(io::ErrorKind::Other, "Unable to read memory"))
                } else {
                    return Ok(trans_size as usize);
                }
            }

            self.convert_hex_data_to_binary(&mut temp_unpack, &temp_buffer[0..len]);
            dest.extend_from_slice(&temp_unpack[0..len/2]);

            // Bump size and prepare for next chunk

            data_size -= current_size;
            addr += current_size;
            trans_size += current_size;

            if data_size == 0 {
                break;
            }
        }

        Ok(trans_size as usize)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::net::{TcpStream, TcpListener};
    use std::sync::{Arc, Mutex};
    use std::io::{Read, Write};
    use std::time::Duration;
    use std::str;

    //const NOT_STARTED: u32 = 0;
    const STARTED: u32 = 1;
    const READ_DATA: u32 = 2;
    const SHOULD_QUIT: u32 = 3;
    //const REPLY_SUPPORT: u32 = 2;

    #[test]
    fn test_checksum_calc() {
        let data: [u8; 8] = [32, 32, 32, 32, 64, 64, 64, 64];
        assert_eq!(::calc_checksum(&data), 128);
    }

    #[test]
    fn test_process_string() {
        let mut dest = String::new();
        GdbRemote::build_processed_string(&mut dest, "f");
        assert_eq!(dest, "$f#66");
    }

    fn check_mutex_complete(mutex: &Arc<Mutex<u32>>) -> bool {
        let data = mutex.lock().unwrap();
        if *data != 0 { true } else { false }
    }

    fn update_mutex(mutex: &Arc<Mutex<u32>>, value: u32) {
        let mut data = mutex.lock().unwrap();
        *data = value
    }

    fn get_mutex_value(mutex: &Arc<Mutex<u32>>) -> u32 {
        *mutex.lock().unwrap()
    }

    fn wait_for_thread_init(mutex: &Arc<Mutex<u32>>) {
        loop {
            if check_mutex_complete(mutex) {
                return;
            }

            thread::sleep(Duration::from_millis(1));
        }
    }

    fn setup_listener(server_lock: &Arc<Mutex<u32>>, state: u32, port: u16) {
        let listener = TcpListener::bind(("127.0.0.1", port)).unwrap();
        update_mutex(&server_lock, state as u32);

        loop {
            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => {
                        server(stream, server_lock);
                        return;
                    }
                    _ => (),
                }
            }
        }
    }

    fn get_string_from_buf(buffer: &[u8], size: usize) -> String {
        String::from_utf8_lossy(&buffer[0..size]).into_owned()
    }

    fn get_string_from_buf_trim(buffer: &[u8], size: usize) -> String {
        String::from_utf8_lossy(&buffer[1..size-3]).into_owned()
    }

    fn parse_memory_req(req: &String) -> (usize, usize) {
        let split: Vec<&str> = req[1..].split(',').collect();
        let addr = usize::from_str_radix(split[0], 16).unwrap();
        let size = usize::from_str_radix(split[1], 16).unwrap();
        (addr, size)
    }

    fn convert_binary_to_hex_data(dest: &mut [u8], src: &[u8]) {
        for (d, s) in dest.chunks_mut(2).zip(src.iter()) {
            d[0] = ::HEX_CHARS[(s >> 4) as usize];
            d[1] = ::HEX_CHARS[(s & 0xf) as usize];
        }
    }

    fn server(mut stream: TcpStream, state: &Arc<Mutex<u32>>) {
        let mut temp_send_data = [0; 4096];
        let mut buffer = [0; 1024];
        let value = get_mutex_value(state);

        // fill in some temp_data used for sending

        for (i, item) in temp_send_data.iter_mut().enumerate().take(4096) {
            *item = (i & 0xff) as u8;
        }

        if value == STARTED {
            return;
        }

        loop {
            if get_mutex_value(state) == SHOULD_QUIT {
                break;
            }

            let len = stream.read(&mut buffer).unwrap();

            if len == 0 {
                continue;
            }

            let data = get_string_from_buf_trim(&buffer, len);

            match data.as_ref() {
                "qSupported" => {
                    let mut dest = String::new();
                    GdbRemote::build_processed_string(&mut dest, "PacketSize=1fff");
                    stream.write(b"+").unwrap(); // reply that we got the package
                    stream.write_all(dest.as_bytes()).unwrap();
                }

                _ => {
                    match buffer[1] {
                        b'm' => {
                            let mut dest = String::new();
                            let (addr, size) = parse_memory_req(&data);
                            convert_binary_to_hex_data(&mut buffer, &temp_send_data[addr..size]);
                            // * 2 in size here because converted from binary -> hex
                            GdbRemote::build_processed_string(&mut dest, str::from_utf8(&buffer[..size*2]).unwrap());
                            stream.write(b"+").unwrap(); // reply that we got the package
                            stream.write_all(dest.as_bytes()).unwrap();
                        },

                        b'g' => {
                            let mut dest = String::new();
                            GdbRemote::build_processed_string(&mut dest, "1122aa");
                            stream.write(b"+").unwrap(); // reply that we got the package
                            stream.write_all(dest.as_bytes()).unwrap();
                        }

                        _ => (),
                    }
                }
            }
        }
    }

    #[test]
    fn test_connect() {
        let port = 6860u16;
        let lock = Arc::new(Mutex::new(0));
        let thread_lock = lock.clone();

        thread::spawn(move || { setup_listener(&thread_lock, STARTED, port) });
        wait_for_thread_init(&lock);

        let mut gdb = GdbRemote::new();
        gdb.connect(("127.0.0.1", port)).unwrap();
    }

    #[test]
    fn test_qsupported() {
        let mut res = [0; 1024];
        let port = 6861u16;
        let lock = Arc::new(Mutex::new(0));
        let thread_lock = lock.clone();

        thread::spawn(move || { setup_listener(&thread_lock, READ_DATA, port) });
        wait_for_thread_init(&lock);

        let mut gdb = GdbRemote::new();
        gdb.connect(("127.0.0.1", port)).unwrap();
        let size = gdb.get_supported(&mut res).unwrap();

        let supported = get_string_from_buf(&res, size);

        assert_eq!(supported, "PacketSize=1fff");

        update_mutex(&lock, SHOULD_QUIT);
    }

    #[test]
    fn test_registers() {
        let mut res = [0; 1024];
        let port = 6862u16;
        let lock = Arc::new(Mutex::new(0));
        let thread_lock = lock.clone();

        thread::spawn(move || { setup_listener(&thread_lock, READ_DATA, port) });
        wait_for_thread_init(&lock);

        let mut gdb = GdbRemote::new();
        gdb.connect(("127.0.0.1", port)).unwrap();
        let size = gdb.get_registers(&mut res).unwrap();

        assert_eq!(res[0], 0x11);
        assert_eq!(res[1], 0x22);
        assert_eq!(res[2], 0xaa);
        assert_eq!(size, 3);

        update_mutex(&lock, SHOULD_QUIT);
    }

    #[test]
    fn test_memory() {
        let mut res = Vec::<u8>::with_capacity(1024);
        let port = 6863u16;
        let lock = Arc::new(Mutex::new(0));
        let thread_lock = lock.clone();

        thread::spawn(move || { setup_listener(&thread_lock, READ_DATA, port) });
        wait_for_thread_init(&lock);

        let mut gdb = GdbRemote::new();
        gdb.connect(("127.0.0.1", port)).unwrap();
        let size = gdb.get_memory(&mut res, 0, 128).unwrap();

        assert_eq!(size, 128);

        for (i, item) in res.iter().enumerate().take(128) {
            assert_eq!(i as u8, *item as u8);
        }

        update_mutex(&lock, SHOULD_QUIT);
    }

    #[test]
    fn test_parse_memory_1() {
        let data = "m77,22".to_owned();
        let (addr, size) = parse_memory_req(&data);
        assert_eq!(addr, 0x77);
        assert_eq!(size, 0x22);
    }

    #[test]
    fn test_parse_memory_2() {
        let data = "m1177,875".to_owned();
        let (addr, size) = parse_memory_req(&data);
        assert_eq!(addr, 0x1177);
        assert_eq!(size, 0x875);
    }

    #[test]
    fn convert_binary_to_hex_data_test() {
        let t = [0x11, 0x23, 0xab];
        let mut res = [0; 6];
        convert_binary_to_hex_data(&mut res, &t);
        let s = get_string_from_buf(&res, 6);
        assert_eq!("1123ab", s);
    }
}

