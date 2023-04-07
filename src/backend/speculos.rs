use std::{net::{TcpStream, Shutdown}};
use std::io::{Read, Write};
use std::convert::TryInto;

use crate::backend::interface::Backend;

const DEFAULT_SPECULOS_PROXY_ADDRESS: &str = "127.0.0.1";
const DEFAULT_SPECULOS_PROXY_PORT: u16 = 9999;

pub struct SpeculosBackend {
    pub server: String,
    pub port: u16,
    pub stream: Option<TcpStream>
}

impl Backend for SpeculosBackend {
    fn new() -> Self {
        SpeculosBackend { server: String::from(DEFAULT_SPECULOS_PROXY_ADDRESS), port: DEFAULT_SPECULOS_PROXY_PORT, stream: None }
    }

    fn open(&mut self) {
        if let Ok(stream) = TcpStream::connect((self.server.as_str(), self.port)) {
            self.stream = Some(stream);
        }
    }

    fn close(&mut self) {
        match &self.stream {
            Some(s) => s.shutdown(Shutdown::Both).unwrap(),
            None => (),
        }
    }

    fn send(&mut self, data: &[u8]) -> std::io::Result<usize> {
        match &mut self.stream {
            Some(s) => {
                let mut data_to_send: Vec<u8> = Vec::from(data.len().to_be_bytes());
                data_to_send.append(&mut Vec::from(data));
                s.write(data_to_send.as_slice())
            }
            None => Ok(0),
        }
    }

    fn recv(&mut self) -> (Vec<u8>, [u8; 2]) {
        match &mut self.stream {
            Some(s) => {
                let mut data_size = [0; 4];
                s.read_exact(&mut data_size).unwrap();
                let size = u32::from_be_bytes(data_size.try_into().unwrap());

                let mut data: Vec<u8>=  vec![0; size as usize];
                s.read(&mut data).unwrap();

                let mut sw = [0; 2];
                s.read(&mut sw).unwrap();

                (data, sw)
            }    
            None => (Vec::<u8>::new(), [0; 2])
        }
    }

}