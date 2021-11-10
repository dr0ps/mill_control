use serialport::{SerialPortType, SerialPortSettings, DataBits, FlowControl, Parity, StopBits, SerialPort};
use std::time::{Duration, Instant};
use std::io::{Write};
use std::ops::Add;
use serde::{Deserialize, Serialize};
use std::thread;
use lazy_static::lazy_static;
use std::sync::{Mutex};
use std::rc::{Weak};
use glib::clone::Downgrade;
use log::{debug,info,error};

lazy_static! {
    static ref LINES_READ : Mutex<Vec<String>> = Mutex::new(vec![]);
    static ref STATUS : Mutex<Status> = Mutex::new(Status {
            line: 0,
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

    static ref QUEUE_FREE : Mutex<u8> = Mutex::new(32);
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
    #[serde(default)] pub line: u32,
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

#[derive(Serialize)]
pub struct StatusFields {
    line: bool,
    posx: bool,
    posy: bool,
    posz: bool,
    posa: bool,
    feed: bool,
    vel: bool,
    unit: bool,
    coor: bool,
    dist: bool,
    frmo: bool,
    stat: bool
}

#[derive(Serialize)]
pub struct SetStatusFields {
    sr: StatusFields
}

#[derive(Deserialize)]
pub struct StatusReport {
    pub sr: Status
}

#[derive(Deserialize)]
pub struct QueueReport {
    pub qr: u8
}

#[derive(Deserialize)]
struct StatusReportResult {
    r: StatusReport,
    f: [u16; 4]
}

fn send_async( port: &mut Box<dyn SerialPort>, message: &str) -> Result<usize, String>
{
    let result = port.write(message.as_bytes());
    if result.is_err()
    {
        return Err(result.err().unwrap().to_string());
    }
    match port.flush()
    {
        Ok(_size) => {

        }
        Err(err) => {
            return Err(err.to_string());
        }
    }
    return Ok(result.unwrap());
}

fn read_async() -> Result<String, String>
{
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

fn send_gcode(port: &mut Box<dyn SerialPort>, code : Box<Vec<String>>)
{
    let mut myp = port.try_clone().unwrap();

    let writer;
    {
        writer = thread::spawn(move || {
            let mut code_iter = code.iter();
            let q = QUEUE_FREE.lock().expect("blah");
            let mut last_queue_free = q.clone();
            drop(q);
            loop {
                let mut queue_free = QUEUE_FREE.lock().expect("blah");
                if *queue_free != last_queue_free
                {
                    last_queue_free = *queue_free;
                }
                if *queue_free > 8
                {
                    let next_line = code_iter.next();

                    let buffer_reduction;

                    match next_line
                    {
                        Some(line) => {
                            let parts:Vec<&str> = line.split(' ').collect();
                            let mut pos = 0;
                            if parts[0].starts_with('N') {
                                pos = 1;
                            }
                            if parts[pos].starts_with("(") {
                                buffer_reduction = 0;
                            }
                            else {
                                buffer_reduction = 2;
                            }
                        }
                        None => {
                            buffer_reduction = 0;
                        }
                    }

                    *queue_free -= buffer_reduction;
                    drop(queue_free);

                    match next_line
                    {
                        Some(line) => {
                            let mut s = String::new();
                            s.push_str("{\"gc\":\"");
                            s.push_str(line);
                            s.push_str("\"}\r\n");

                            send_async(&mut myp, &s).expect("Failed to send async.");
                        }
                        None => {
                            break;
                        }
                    }
                }
                else {
                    drop(queue_free);
                }
                thread::sleep(Duration::from_nanos(10));
            }
        });
    }

    let reader = thread::spawn(move ||  {
        loop {
            match read_async()
            {
                Ok(_msg) => {
                }
                Err(msg) => {
                    if msg.eq("Timeout.")
                    {
                        debug!("Timeout.");
                    }
                    else
                    {
                        error!("Timeout.");
                        break;
                    }
                }
            }
            thread::sleep(Duration::from_nanos(10));
        }
    });

    writer.join().unwrap();
    reader.join().unwrap();
}

fn send( port: &mut Box<dyn SerialPort>, message: &str) -> Result<String, String>
{
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
                SerialPortType::UsbPort(info) => {
                    match info.product.as_deref() {
                        Some("FT230X_Basic_UART") => {
                            tinyg_ports.push(p.port_name);
                        }
                        Some(_str) => {

                        }
                        None => {

                        }
                    }
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
        info!("Using port {}", tinyg_ports.get(0).unwrap());
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
                                        let mut line  = String::from(result.clone()[0..char_at.unwrap().0].trim());
                                        line.retain(|c| c != 0x13 as char && c != 0x11 as char);
                                        LINES_READ.lock().expect("blah!").push(line);
                                        result = String::from(result.split_off(char_at.unwrap().0+1));
                                        chars = result.char_indices();
                                    }
                                    char_at = chars.next();
                                }
                            }
                            else {
                                let mut line  = String::from(&result.clone()[0..start as usize]);
                                line.retain(|c| c != 0x13 as char && c != 0x11 as char);
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
                                        let char_at = chars.next();
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
                                        let mut sub = String::from(sub.trim());
                                        sub.retain(|c| c != 0x13 as char && c != 0x11 as char);
                                        result = String::from(result.split_off(end));

                                        if sub.starts_with("{\"sr\":") {
                                            let status: StatusReport = serde_json::from_str(sub.as_str()).expect(format!("Unable to run serde with this input: >{}<", sub).as_str());
                                            *STATUS.lock().expect("blah!") = status.sr.clone();
                                        }
                                        else if sub.starts_with("{\"qr\":") {
                                            let status: QueueReport = serde_json::from_str(sub.as_str()).expect(format!("Unable to run serde with this input: >{}<", sub).as_str());
                                            *QUEUE_FREE.lock().expect("blah!") = status.qr.clone();
                                        }
                                        else {
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
                    debug!("result: {}",result);
                    if result.trim().contains(&String::from("tinyg [mm] ok>"))
                    {
                        info!("Init received {}", result);
                        initialized = true;
                        break;
                    }
                    else
                    {
                        debug!("other {}", result.trim());
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
                    debug!("Received {}",result);
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

    pub fn set_status_fields(&mut self) -> Result<StatusReport, String> {
        let fields = StatusFields{line: true, coor: true, dist: true, feed: true, frmo: true, posa: true, posx: true, posy: true, posz: true, stat: true, unit: true, vel: true};
        let set_fields = SetStatusFields{sr: fields};
        let result = send(self.port.as_mut().expect(""), serde_json::to_string(&set_fields).unwrap().add("\r\n").as_str())?;
        let status_report: StatusReportResult = serde_json::from_str(result.as_str()).unwrap();
        Ok(status_report.r)
    }

    pub fn get_status(&mut self) -> Result<StatusReport, String> {
        let verbosity = SetVerbosity{sv:2};
        let _result = send(self.port.as_mut().expect(""), serde_json::to_string(&verbosity).unwrap().add("\r\n").as_str())?;
        let result = send(self.port.as_mut().expect(""), "{\"sr\":null}\r\n")?;
        let status_report: StatusReportResult = serde_json::from_str(result.as_str()).unwrap();
        Ok(status_report.r)
    }

    pub fn send_config(&mut self, config : String) -> Result<String, String> {
        let mut msg = String::from("{");
        msg.push_str(config.as_str());
        msg.push_str("}\r\n");

        let result = send(self.port.as_mut().expect(""), msg.as_str())?;
        Ok(result)
    }

    pub fn home_all(&mut self) -> Result<String, String> {
        let result = send(self.port.as_mut().expect(""), "{\"gc\":\"g28.2 z0 y0 x0\"}\r\n")?;
        Ok(result)
    }

    pub fn move_xyza(&mut self, x: Option<f32>, y: Option<f32>, z: Option<f32>, a: Option<f32>) -> Result<String, String> {
        let mut msg = String::from("{\"gc\":\"g91 g0");
        if x.is_some()
        {
            msg.push_str(" ");
            msg.push_str("x");
            msg.push_str(x.unwrap().to_string().as_str());
        }
        if y.is_some()
        {
            msg.push_str(" ");
            msg.push_str("y");
            msg.push_str(y.unwrap().to_string().as_str());
        }
        if z.is_some()
        {
            msg.push_str(" ");
            msg.push_str("z");
            msg.push_str(z.unwrap().to_string().as_str());
        }
        if a.is_some()
        {
            msg.push_str(" ");
            msg.push_str("a");
            msg.push_str(a.unwrap().to_string().as_str());
        }
        msg.push_str("\"}\r\n");
        let result = send(self.port.as_mut().expect(""), msg.as_str())?;
        Ok(result)
    }

    pub fn zero_x(&mut self) -> Result<String, String> {
        let result = send(self.port.as_mut().expect(""), "{\"gc\":\"g92 x0\"}\r\n")?;
        Ok(result)
    }

    pub fn zero_y(&mut self) -> Result<String, String> {
        let result = send(self.port.as_mut().expect(""), "{\"gc\":\"g92 y0\"}\r\n")?;
        Ok(result)
    }

    pub fn zero_z(&mut self) -> Result<String, String> {
        let result = send(self.port.as_mut().expect(""), "{\"gc\":\"g92 z0\"}\r\n")?;
        Ok(result)
    }

    pub fn zero_a(&mut self) -> Result<String, String> {
        let result = send(self.port.as_mut().expect(""), "{\"gc\":\"g92 a0\"}\r\n")?;
        Ok(result)
    }

    pub fn cycle_start(&mut self) {
        send_async(self.port.as_mut().expect(""), "~\r\n").expect("Failed to send cycle start.");
    }

    pub fn feed_hold(&mut self) {
        send_async(self.port.as_mut().expect(""), "!\r\n").expect("Failed to send feed hold.");
    }

    pub fn send_gcode(&mut self, code : Box<Vec<String>>)
    {
        send_gcode(self.port.as_mut().expect(""), code);
        return;
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