use serialport::{SerialPortType, SerialPortSettings, DataBits, FlowControl, Parity, StopBits, SerialPort};
use std::time::{Duration, Instant};
use std::io::{stdout, Write};
use std::ops::Add;
use serde::{Deserialize, Serialize};

pub struct Tinyg {
    port : Option<Box<dyn SerialPort>>
}

#[derive(Serialize)]
struct SetVerbosity {
    sv: u16,
}

#[derive(Deserialize)]
pub struct Status {
    #[serde(default)] pub posx: f32,
    #[serde(default)] pub posy: f32,
    #[serde(default)] pub posz: f32,
    #[serde(default)] pub posa: f32,
    #[serde(default)] pub feed: f32,
    #[serde(default)] pub vel: f32,
    #[serde(default)] pub unit: u8,
    #[serde(default)] pub coor: u8,
    #[serde(default)] pub dist: u8,
    #[serde(default)] pub frmo: u8,
    #[serde(default)] pub stat: u8
}

#[derive(Deserialize)]
pub struct StatusReport {
    pub sr: Status
}

#[derive(Deserialize)]
struct StatusReportResult {
    r: StatusReport,
    f: [u16; 4]
}

fn send( port: &mut Box<dyn SerialPort>, message: &str) -> Result<String, String>
{
    println!("Sending >{}<", message);
    stdout().flush().unwrap();
    match port.write(message.as_bytes())
    {
        Ok(_size) => {

        }
        Err(err) => {
            return Err(err.to_string());
        }
    }
    match port.flush()
    {
        Ok(_size) => {

        }
        Err(err) => {
            return Err(err.to_string());
        }
    }
    let mut result = String::new();

    let mut start = Instant::now();

    loop {
        match port.bytes_to_read() {
            Ok(bytes) => {
                if bytes == 0 {
                    if start.elapsed().as_millis() > 100
                    {
                        break;
                    }
                }
                else {
                    let mut buffer = [0u8; 4096];
                    match port.read(&mut buffer)
                    {
                        Ok(size) => {
                            result = result.add(String::from_utf8_lossy(&buffer[0..size]).trim());
                        }
                        Err(err) => {
                            return Err(err.to_string());
                        }
                    }
                    start = Instant::now();
                }
            }
            Err(err) => {
                return Err(err.to_string());
            }
        }
    }
    let mut final_result= None;
        for line in result.lines() {
            println!("Line: >{}<", line);
            if line.starts_with("{\"r\":") {
                final_result = Some(String::from(line));
            }
            else if line.starts_with("{\"sr\":") {
                let status: StatusReport = serde_json::from_str(line).unwrap();
            }
            else if (line.starts_with("tinyg [mm] ok>"))
            {
                final_result = Some(String::from(line));
            }
        }
    return Ok(final_result.expect("Has to be here!"));
}

impl Tinyg {
    pub fn new() -> Self {
        Self { port:None }
    }

    pub fn initialize(&mut self) -> Result<(), String> {
        let ports = serialport::available_ports().expect("No ports found!");
        let mut tinyg_ports = Vec::new();
        for p in ports {
            match p.port_type {
                SerialPortType::UsbPort(_info) => {
                    tinyg_ports.push(p.port_name);
                }
                SerialPortType::BluetoothPort => {
                }
                SerialPortType::PciPort => {
                }
                SerialPortType::Unknown => {
                }
            }
        }
        if tinyg_ports.is_empty() {
            return Err(String::from("No port found."))
        }
        println!("Using port {}", tinyg_ports.get(0).unwrap());
        stdout().flush().unwrap();
        let tinyg_port = tinyg_ports.get(0).unwrap();
        let s = SerialPortSettings {
            baud_rate: 115200,
            data_bits: DataBits::Eight,
            flow_control: FlowControl::Hardware,
            parity: Parity::None,
            stop_bits: StopBits::One,
            timeout: Duration::from_millis(500),
        };
        let mut port = serialport::open_with_settings(tinyg_port, &s).expect("Failed to open serial port");

        let mut initialized= false;
        for _x in 0..10 {
            match send(&mut port, "$\r\n") {
                Ok(result) => {
                    if result.trim().contains(&String::from("tinyg [mm] ok>"))
                    {
                        println!("Init received {}", result);
                        stdout().flush().unwrap();
                        initialized = true;
                        break;
                    }
                    else
                    {
                        println!("other {}", result.trim());
                        stdout().flush().unwrap();
                    }

                }
                Err(err) => {
                    return Err(err);
                }
            }
        }

        if initialized
        {
            match send(&mut port, "{ej:1}\r\n") {
                Ok(result) => {
                    println!("Received {}",result);
                    stdout().flush().unwrap();
                }
                Err(err) => {
                    return Err(err);
                }
            }

            self.port = Some(port);
            Ok(())
        }
        else
        {
            Err(String::from("Unable to initialize."))
        }
    }



    pub fn get_system_status(&mut self) -> Result<String, String> {
        let result = send(self.port.as_mut().expect(""), "{\"sys\":null}\r\n")?;
        Ok(result)
    }

    pub fn get_status(&mut self) -> Result<StatusReport, String> {
        let verbosity = SetVerbosity{sv:2};
        let _result = send(self.port.as_mut().expect(""), serde_json::to_string(&verbosity).unwrap().add("\r\n").as_str())?;
        let result = send(self.port.as_mut().expect(""), "{\"sr\":null}\r\n")?;
        let status_report: StatusReportResult = serde_json::from_str(result.as_str()).unwrap();
        Ok(status_report.r)
    }
}