use serialport::{SerialPortType, SerialPortSettings, DataBits, FlowControl, Parity, StopBits, SerialPort};
use std::time::{Duration, Instant};
use std::io::{stdout, Write};
use std::ops::Add;
use serde::{Deserialize, Serialize};
use std::thread;
use lazy_static::lazy_static;
use std::sync::{Mutex};
use std::rc::{Weak};
use glib::clone::Downgrade;

lazy_static! {
    static ref LINES_READ : Mutex<Vec<String>> = Mutex::new(vec![]);
    static ref STATUS : Mutex<Status> = Mutex::new(Status {
            posx: 0.0,
            posy: 0.0,
            posz: 0.0,
            posa: 0.0,
            feed: 0.0,
            vel: 0.0,
            unit: 0,
            coor: 0,
            dist: 0,
            frmo: 0,
            stat: 0
        });
}

pub struct Tinyg {
    port : Option<Box<dyn SerialPort>>,
}

#[derive(Serialize)]
struct SetVerbosity {
    sv: u16,
}

#[derive(Deserialize, Clone, Copy)]
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
    println!("Sending {}", message);
    let json_only = message.starts_with('{');

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


    let start = Instant::now();

    let mut final_result= None;
    loop {
        let mut lines = LINES_READ.lock().expect("blah!");
        if !lines.is_empty() {
            let prefix : Vec<String> = lines.drain(0..1).collect();
            let line = prefix.first().expect("Vector was not empty.");
            if line.starts_with("{\"r\":") {
                final_result = Some(String::from(line));
            }
            else if !json_only && line.starts_with("tinyg [mm] ok>")
            {
                final_result = Some(String::from(line));
            }
            if final_result.is_some() {
                break;
            }
        } else {
            if start.elapsed().as_millis() > 1000
            {
                return Err(String::from("Timeout."));
            }
        }
    }

    return Ok(final_result.expect("Has to be here!"));
}

impl Tinyg {
    pub fn new() -> Self {
        Self { port:None }
    }

    pub fn get_latest_status(&mut self) -> Result<Status, String>
    {
        let status = *STATUS.lock().expect("blah!");
        return Ok(status);
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

        {
            let mut port_clone = port.try_clone().expect("Has to be able to clone");
            thread::spawn(move || {
                let mut result = String::new();
                loop {
                    let mut buffer = [0u8; 4096];
                    match port_clone.read(&mut buffer)
                    {
                        Ok(size) => {
                            result = result.add(String::from_utf8_lossy(&buffer[0..size]).trim());
                            println!("Current buffer length: {}", result.len());
                            let mut start : i32 = -1;
                            let mut chars = result.char_indices();
                            let mut char_at = chars.next();
                            'first_brace: while char_at.is_some() {
                                if char_at.unwrap().1 == '{' {
                                    start = char_at.unwrap().0 as i32;
                                    break 'first_brace;
                                }
                                char_at = chars.next();
                            }
                            if start == -1
                            {
                                let mut chars = result.char_indices();
                                let mut char_at = chars.next();
                                while char_at.is_some() {
                                    if char_at.unwrap().1 == '\n' {
                                        let line  = String::from(result.clone()[0..char_at.unwrap().0].trim());
                                        LINES_READ.lock().expect("blah!").push(line);
                                        result = String::from(result.split_off(char_at.unwrap().0+1));
                                        chars = result.char_indices();
                                    }
                                    char_at = chars.next();
                                }
                            }
                            else {
                                let line  = String::from(&result.clone()[0..start as usize]);
                                if line.trim().len() > 0
                                {
                                    LINES_READ.lock().expect("blah!").push(String::from(line.trim()));
                                }
                                result = String::from(result.split_off(start as usize).trim());

                                'json: while result.len() > 0 {
                                    let mut end = 0;
                                    let mut open_brace_count = 1;
                                    let mut chars = result.char_indices();
                                    'brace_count: while open_brace_count > 0 {
                                        let mut char_at = chars.next();
                                        if char_at.is_some() {
                                            if char_at.unwrap().1 == '}' {
                                                open_brace_count = open_brace_count - 1;
                                                if open_brace_count == 0
                                                {
                                                    end = char_at.unwrap().0 + 1;
                                                }
                                            }
                                            if char_at.unwrap().1 == '{' && char_at.unwrap().0 > 1 {
                                                open_brace_count = open_brace_count + 1;
                                            }
                                        } else {
                                            break 'brace_count; //incomplete
                                        }
                                    }
                                    if open_brace_count == 0 {
                                        let sub = &result.clone()[0..end];
                                        let sub = sub.trim();
                                        result = String::from(result.split_off(end));

                                        if sub.starts_with("{\"sr\":") {
                                            let status: StatusReport = serde_json::from_str(sub).expect(format!("Unable to run serde with this input: >{}<", sub).as_str());
                                            *STATUS.lock().expect("blah!") = status.sr.clone();
                                        }
                                        else {
                                            println!("Passing {}", sub);
                                            LINES_READ.lock().expect("blah!").push(String::from(sub));
                                        }
                                    } else {
                                        break 'json;
                                    }
                                }
                            }
                        }
                        Err(_err) => {}
                    }
                }
            });
        }

        let mut initialized= false;
        for _x in 0..10 {
            match send(&mut port, "$\r\n") {
                Ok(result) => {
                    println!("result: {}",result);
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
        println!("Parsing >{}<", result);
        let status_report: StatusReportResult = serde_json::from_str(result.as_str()).unwrap();
        Ok(status_report.r)
    }

    pub fn home_all(&mut self) -> Result<String, String> {
        let result = send(self.port.as_mut().expect(""), "{\"gc\":\"g28.2 x0 y0 z0\"}\r\n")?;
        Ok(result)
    }
}

impl Clone for Tinyg {
    fn clone(&self) -> Self {
        let port = self.port.as_ref().unwrap().try_clone().unwrap();
        return Self {port : Option::Some(port)};
    }
}

impl Downgrade for Tinyg {

    type Weak = Weak<Tinyg>;

    fn downgrade(&self) -> Weak<Tinyg> {
        Weak::new()
    }
}