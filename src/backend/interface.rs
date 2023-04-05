pub trait Backend {

    fn new() -> Self;
    
    fn open(&mut self);

    fn close(&mut self);

    fn send(&mut self, data: &[u8]) -> std::io::Result<usize>;

    fn recv(&mut self) -> (Vec<u8>, [u8; 2]);

}