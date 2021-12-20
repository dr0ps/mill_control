use crate::tinyg::{Tinyg};
use hidapi::HidApi;
use std::thread;
use log::{debug, error};

pub struct Whb04b {
}

fn get_bytes(input : f32) -> [u8; 4] {
    let mut output = [0u8; 4];
    let integer_part = input.abs().trunc() as u16;

    let mut fractional_part = ((input.abs() - input.abs().trunc()) * 10000.0) as u16;

    if input < 0.0 {
        fractional_part = fractional_part | 0x8000;
    }

    output[0] = integer_part.to_be_bytes()[0];
    output[1] = integer_part.to_be_bytes()[1];
    output[2] = fractional_part.to_be_bytes()[0];
    output[3] = fractional_part.to_be_bytes()[1];
    output
}

impl Whb04b {

    pub fn initialize<F>(f: F) where F: Fn() -> Tinyg + std::marker::Sync + 'static + std::marker::Send {
        thread::spawn(move || {
            match HidApi::new() {
                Ok(api) => {
                    for device in api.device_list() {
                        debug!("{:04x}:{:04x}, {}", device.vendor_id(), device.product_id(), device.manufacturer_string().or(Some("None")).unwrap());
                        if device.product_id() == 0xeb93 {
                            match device.open_device(&api){
                                Ok(device) => {
                                    loop {
                                        let mut input_buf: [u8; 8] = [0; 8];
                                        let axis;
                                        if device.read_timeout(&mut input_buf, -1).is_ok() {
                                            //println!("Read {:#04?}", input_buf);
                                            let feed = input_buf[4];
                                            axis = input_buf[5];
                                            let delta = input_buf[6] as i8;
                                            let distance = match feed {
                                                13 => delta as f32 * 0.001,
                                                14 => delta as f32 * 0.01,
                                                15 => delta as f32 * 0.1,
                                                16 => delta as f32,
                                                _ => 0.0
                                            };

                                            if delta != 0 {
                                                match f().move_xyza(
                                                    if axis == 17 { Some(distance) } else { None },
                                                    if axis == 18 { Some(distance) } else { None },
                                                    if axis == 19 { Some(distance) } else { None },
                                                    if axis == 20 { Some(distance) } else { None }) {
                                                    Ok(_) => {}
                                                    Err(error) => error!("Error in Tinyg.move_xyza:  {}", error)
                                                }
                                            }

                                            let status = f().get_latest_status().unwrap();
                                            let pos_x = if axis < 20 { get_bytes(status.posx) } else { get_bytes(status.posa) };
                                            let pos_y = if axis < 20 { get_bytes(status.posy) } else { [0; 4] };
                                            let pos_z = if axis < 20 { get_bytes(status.posz) } else { [0; 4] };

                                            let output_buf: [u8; 8] = [0x06, 0xFE, 0xFD, 0xFF, 0x00, pos_x[1], pos_x[0], pos_x[3]];
                                            match device.send_feature_report(&output_buf) {
                                                Ok(_) => {}
                                                Err(error) => error!("Error in HidDevice.send_feature_report: {}", error)
                                            }
                                            let output_buf: [u8; 8] = [0x06, pos_x[2], pos_y[1], pos_y[0], pos_y[3], pos_y[2], pos_z[1], pos_z[0]];
                                            match device.send_feature_report(&output_buf) {
                                                Ok(_) => {}
                                                Err(error) => error!("Error in HidDevice.send_feature_report: {}", error)
                                            }
                                            let output_buf: [u8; 8] = [0x06, pos_z[3], pos_z[2], 0x00, 0x00, 0x00, 0x00, 0x00];
                                            match device.send_feature_report(&output_buf) {
                                                Ok(_) => {}
                                                Err(error) => error!("Error in HidDevice.send_feature_report: {}", error)
                                            }
                                        }
                                    }
                                }
                                Err(error) => {
                                    error!("Unable to open hid device: {}", error);
                                }
                            }
                        }
                    }
                },
                Err(e) => {
                    error!("Error: {}", e);
                },
            }
        });
    }
}
