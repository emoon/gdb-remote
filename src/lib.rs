use std::net::TcpStream;
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
        // 2 sec of time-out to make sure we never gets infitite blocked
        try!(stream.set_read_timeout(Some(Duration::from_secs(2))));
        self.stream = Some(stream);
        self.connect_internal()
    }

    pub fn connect_internal(&mut self) -> io::Result<()> {
        // Handshake with the server
    
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

    fn handle_send_ack(stream: &mut TcpStream, resend_data: &String, dest: &mut [u8]) -> io::Result<usize> {
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

    fn validate_checksum(data: &[u8], len: usize) -> io::Result<usize> {
        // - 3 to skip $xx in the end
        let data_len = len;
        let checksum = calc_checksum(&data[1..len]);
        let data_check = from_pair_hex((data[data_len - 2], data[data_len - 1]));

        if data_check == checksum {
            Ok((data_len))
        } else {
            Err(io::Error::new(io::ErrorKind::Other, "Checksum missmatch for data"))
        }
    }

    pub fn send_command_wait_reply_raw(&mut self, res: &mut [u8], command: &str) -> io::Result<usize> {
        let mut temp_buffer = [0; PACKET_SIZE];
        try!(self.send_command(command));
        let len = try!(self.read_reply(&mut temp_buffer));
        try!(Self::validate_checksum(&temp_buffer, len));
        // skip start and checksum data
        res.clone_from_slice(&temp_buffer[1..len-3]);
        Ok((len - 3))
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
#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::net::{TcpStream, TcpListener};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[allow(dead_code)] 
    enum ServerState {
        NotStarted,
        Started,
        ReplyCorrect,
    }

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

    fn wait_for_thread_init(mutex: &Arc<Mutex<u32>>) {
        loop {
            if check_mutex_complete(mutex) {
                return;
            }

            thread::sleep(Duration::from_millis(1));
        }
    }

    fn setup_listener(server_lock: &Arc<Mutex<u32>>, state: ServerState) {
        let listener = TcpListener::bind("127.0.0.1:6860").unwrap();
        update_mutex(&server_lock, state as u32);

        loop {
            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => server(stream, server_lock),
                    _ => (),
                }
            }
        }
    }

    fn server(_stream: TcpStream, _state: &Arc<Mutex<u32>>) {
        loop {
            // do severy things here
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn test_connect() {
        let lock = Arc::new(Mutex::new(0));
        let thread_lock = lock.clone();

        thread::spawn(move || { setup_listener(&thread_lock, ServerState::Started) });
        // make sure we have spawned the server before we go on
        wait_for_thread_init(&lock);

        let mut gdb = GdbRemote::new();
        gdb.connect("127.0.0.1:6860").unwrap();
    }
}

