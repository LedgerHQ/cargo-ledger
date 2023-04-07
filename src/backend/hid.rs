use hidapi::{HidApi, DeviceInfo, HidDevice};

use crate::backend::interface::Backend;
pub struct HidBackend {
    hid: HidApi,
    device_info: Option<DeviceInfo>,
    device: Option<HidDevice>
}

impl HidBackend {
    pub fn new() -> Self {
        let api = hidapi::HidApi::new().unwrap();
        HidBackend {
            hid: api,
            device_info: None,
            device: None
        }
    }
}

impl Backend for HidBackend {
    fn open(&mut self) {

        let device_info = self.hid.device_list().find(
            |&device| device.vendor_id() == 0x2C97 && 
            (device.interface_number() == 0 || device.usage_page() == 0xffa0));

        match device_info {
            Some(d) => {
                self.device_info = Some(d.clone());
                let device = d.open_device(&self.hid);
                match device {
                    Ok(d) => {
                        d.set_blocking_mode(false).unwrap();
                        self.device = Some(d);
                    }
                    Err(_e) => ()
                }
            },
            None => ()
        }
    }

    fn close(&mut self) {}

    fn send(&mut self, data: &[u8]) -> std::io::Result<usize> {

        let len: u16 = data.len() as u16;
        let data_to_send = [&len.to_be_bytes()[..], data].concat(); 

        let mut offset = 0;
        let mut seq_idx: u16 = 0;
        let mut header: Vec<u8>;
        let zero: Vec<u8> = vec![0x00]; // report ID

        match &self.device {
            Some(device) => {
                while offset < data_to_send.len() {
                    // Header: channel (0x101), tag (0x05), sequence index
                    let seq_idx_bytes = seq_idx.to_be_bytes(); 
                    header = vec![0x01, 0x01, 0x05, seq_idx_bytes[0], seq_idx_bytes[1]];
                    let chunk = [
                        &zero[..], 
                        &header[..], 
                        &data_to_send[offset..std::cmp::min(offset + 64 - header.len(), data_to_send.len())]
                    ].concat();
                    match device.write(chunk.as_slice()) {
                        Ok(_s) => (),
                        Err(e) => {
                            eprintln!("HID write error {}", e);
                            return Ok(offset);
                        }
                    }
                    offset += 64 - header.len();
                    seq_idx += 1;
                }
                Ok(data.len())
            }
            None => {
                eprintln!("No Ledger device");
                Ok(0)
            }
        }
    }

    fn recv(&mut self) -> (Vec<u8>, [u8; 2]) {

        let mut chunk: [u8; 65] = [0;65];

        match &self.device {
            Some(device) => {

                device.set_blocking_mode(true).unwrap();
                device.read(&mut chunk[..]).unwrap();
                device.set_blocking_mode(false).unwrap();

                let data_len: usize = (chunk[5] * 255) as usize + chunk[6] as usize;
                
                let mut data: Vec<u8> = Vec::new();
                data.extend_from_slice(&chunk[7..std::cmp::min(64, 7 + data_len)]);

                
                while data.len() < data_len {
                    device.read_timeout(&mut chunk[..], 1000).unwrap();
                    data.extend_from_slice(&chunk[5..std::cmp::min(64, 5 + data_len - data.len())]);
                }
                
                let mut sw: [u8; 2] = [0;2];
                sw[1] = data.pop().unwrap();
                sw[0] = data.pop().unwrap();
                
                (data, sw)
            }
            None => {
                eprintln!("No Ledger device");
                (Vec::new(), [0; 2])
            }
        }
    }
}