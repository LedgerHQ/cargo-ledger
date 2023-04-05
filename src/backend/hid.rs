use crate::backend::interface::Backend;
pub struct HidBackend {
}

impl Backend for HidBackend {
    fn new() -> Self {
        HidBackend { }
    }

    fn open(&mut self) {
    }

    fn close(&mut self) {
    }

    fn send(&mut self, data: &[u8]) -> std::io::Result<usize> {
        Ok(0)
    }

    fn recv(&mut self) -> (Vec<u8>, [u8; 2]) {
         (Vec::<u8>::new(), [0; 2])
    }
}