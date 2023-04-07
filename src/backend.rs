use clap::{ValueEnum};

mod speculos;
mod hid;
mod interface;

use interface::Backend;
use speculos::SpeculosBackend;
use hid::HidBackend;

#[derive(ValueEnum, Clone, Debug)]
pub enum BackendType {
    Speculos,
    Hid,
}

impl AsRef<str> for BackendType {
    fn as_ref(&self) -> &str {
        match self {
            BackendType::Speculos => "speculos",
            BackendType::Hid => "hid"
        }
    }
}

pub struct Comm {
    pipe: Box<dyn Backend>,
}

impl Comm {
    pub fn create(backend: BackendType) -> Self {
        let mut comm: Comm = match backend {
            BackendType::Speculos => {
                let pipe = SpeculosBackend::new();
                Comm {
                    pipe: Box::new(pipe)
                }
            }
            BackendType::Hid => {
                let pipe = HidBackend::new();
                Comm {
                    pipe: Box::new(pipe)
                }
            }
        };
        comm.pipe.open();
        comm
    }

    pub fn exchange_apdu(&mut self, apdu: &[u8]) -> (Vec<u8>, [u8; 2]) {
        self.pipe.send(apdu).unwrap();
        self.pipe.recv()
    }
}