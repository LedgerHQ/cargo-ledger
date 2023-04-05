mod speculos;
mod hid;
mod interface;

pub use interface::Backend;
pub use speculos::SpeculosBackend;
pub use hid::HidBackend;

pub struct Comm<T: Backend> {
    pipe: T,
}

impl <T: Backend>Comm<T> {
    pub fn create() -> Self {
        let mut pipe = T::new();
        pipe.open();
        Comm {
            pipe
        }
    }

    pub fn exchange_apdu(&mut self, apdu: &[u8]) -> (Vec<u8>, [u8; 2]) {
        self.pipe.send(apdu).unwrap();
        self.pipe.recv()
    }
}