use std::process::exit;
use crate::tinyg::{Tinyg};
use std::io::{stdout, Write, BufReader, Read, BufRead};
use std::io;

extern crate gdk;
extern crate gtk;
extern crate gio;

use gtk::prelude::*;
use glib::{clone};
use std::fs::File;

use std::thread;
use std::time;
use std::path::Path;

use lazy_static::lazy_static;
use std::sync::{Mutex};

mod tinyg;

enum Message {
    UpdateLabel(tinyg::Status),
}

lazy_static! {
    static ref TINY_G : Mutex<Tinyg> = Mutex::new(Tinyg::new());
    static ref TINY_G2 : Mutex<Tinyg> = Mutex::new(TINY_G.lock().unwrap().clone());
}

fn read_lines<P>(filename: P) -> io::Result<io::Lines<io::BufReader<File>>>
    where P: AsRef<Path>, {
    let file = File::open(filename)?;
    Ok(io::BufReader::new(file).lines())
}

pub fn main() {
    match TINY_G.lock().expect("Unable to lock Tiny-G").initialize() {
        Ok(()) => {
            println!("Initialization complete.");
            stdout().flush().unwrap();
        }
        Err(error) => {
            println!("Error: {}", error);
            exit(0);
        }
    }
    match TINY_G.lock().expect("Unable to lock Tiny-G").get_system_status() {
        Ok(result) => {
            println!("Status: {}", result);
            stdout().flush().unwrap();
        }
        Err(error) => {
            println!("Error: {}", error);
            exit(0);
        }
    }

    if let Ok(lines) = read_lines("./config") {
        for line in lines {
            if let Ok(cfg) = line {
                match TINY_G.lock().expect("Unable to lock Tiny-G").send_config(cfg)
                {
                    Ok(result) => {
                        println!("Status: {}", result);
                        stdout().flush().unwrap();
                    }
                    Err(error) => {
                        println!("Error: {}", error);
                        exit(0);
                    }
                }
            }
        }
    }

    match TINY_G.lock().expect("Unable to lock Tiny-G").get_status() {
        Ok(result) => {
            println!("Status: {}", result.sr.stat);
            stdout().flush().unwrap();
        }
        Err(error) => {
            println!("Error: {}", error);
            exit(0);
        }
    }

    if gtk::init().is_err() {
        println!("Failed to initialize GTK.");
        return;
    }

    let style = include_str!("../style.css");
    let provider = gtk::CssProvider::new();
    provider
        .load_from_data(style.as_bytes())
        .expect("Failed to load CSS");
    // We give the CssProvided to the default screen so the CSS rules we added
    // can be applied to our window.
    gtk::StyleContext::add_provider_for_screen(
        &gdk::Screen::get_default().expect("Error initializing gtk css provider."),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let glade_src = include_str!("../mainwindow.glade");
    let builder = gtk::Builder::from_string(glade_src);

    let main_window: gtk::Window = builder.get_object("main_window").unwrap();
    let text_view: gtk::TextView = builder.get_object("gcode_view").unwrap();

    let file_choose_button : gtk::FileChooserButton = builder.get_object("file_choose_button").unwrap();
    file_choose_button.connect_file_set(clone!(@weak text_view => move |file_choose_button| {
        let filename = file_choose_button.get_filename().expect("Couldn't get filename");
        let file = File::open(&filename).expect("Couldn't open file");

        let mut reader = BufReader::new(file);
        let mut contents = String::new();
        let _ = reader.read_to_string(&mut contents);

        text_view
            .get_buffer()
            .expect("Couldn't get window")
            .set_text(&contents);

        let mut gcode_lines : Vec<String> = Vec::new();
        contents.lines().for_each(
            |x| {
                if x.starts_with('(') {
                    println!("Discarded: {}", x);
                }
                else {
                    let mut s = String::new();
                    s.push_str("{\"gc\":\"");
                    s.push_str(x.splitn(2, ';').next().unwrap().trim());
                    s.push_str("\"}\r\n");
                    gcode_lines.push(s);
                }
            }
        );

        thread::spawn(move || {
            let ting_ref = TINY_G.lock().expect("Unable to lock Tiny-G");
            let mut tinyg = ting_ref.clone();
            drop(ting_ref);

            tinyg.send_gcode(Box::new(gcode_lines), |x| {});
        });
    }));

    builder.get_object::<gtk::Button>("refallhome_button").unwrap().connect_clicked(|_button| { TINY_G.lock().expect("Unable to lock Tiny-G").home_all(); });

    builder.get_object::<gtk::Button>("zerox_button").unwrap().connect_clicked(|_button| { TINY_G.lock().expect("Unable to lock Tiny-G").zero_x(); });
    builder.get_object::<gtk::Button>("zeroy_button").unwrap().connect_clicked(|_button| { TINY_G.lock().expect("Unable to lock Tiny-G").zero_y(); });
    builder.get_object::<gtk::Button>("zeroz_button").unwrap().connect_clicked(|_button| { TINY_G.lock().expect("Unable to lock Tiny-G").zero_z(); });
    builder.get_object::<gtk::Button>("zeroa_button").unwrap().connect_clicked(|_button| { TINY_G.lock().expect("Unable to lock Tiny-G").zero_a(); });
    builder.get_object::<gtk::Button>("cycle_start_button").unwrap().connect_clicked(|_button| { TINY_G2.lock().expect("Unable to lock Tiny-G").cycle_start(); });
    builder.get_object::<gtk::Button>("feed_hold_button").unwrap().connect_clicked(|_button| { TINY_G2.lock().expect("Unable to lock Tiny-G").feed_hold(); });

    builder.get_object::<gtk::Button>("x_minus_button").unwrap().connect_clicked(|_button| { TINY_G.lock().expect("Unable to lock Tiny-G").move_xyza(Some(-1.0), None, None, None); });
    builder.get_object::<gtk::Button>("x_plus_button").unwrap().connect_clicked(|_button| { TINY_G.lock().expect("Unable to lock Tiny-G").move_xyza(Some(1.0), None, None, None); });
    builder.get_object::<gtk::Button>("y_minus_button").unwrap().connect_clicked(|_button| { TINY_G.lock().expect("Unable to lock Tiny-G").move_xyza(None, Some(-1.0), None, None); });
    builder.get_object::<gtk::Button>("y_plus_button").unwrap().connect_clicked(|_button| { TINY_G.lock().expect("Unable to lock Tiny-G").move_xyza(None, Some(1.0), None, None); });
    builder.get_object::<gtk::Button>("x_minus_y_minus_button").unwrap().connect_clicked(|_button| { TINY_G.lock().expect("Unable to lock Tiny-G").move_xyza(Some(-1.0), Some(-1.0), None, None); });
    builder.get_object::<gtk::Button>("x_minus_y_plus_button").unwrap().connect_clicked(|_button| { TINY_G.lock().expect("Unable to lock Tiny-G").move_xyza(Some(-1.0), Some(1.0), None, None); });
    builder.get_object::<gtk::Button>("x_plus_y_minus_button").unwrap().connect_clicked(|_button| { TINY_G.lock().expect("Unable to lock Tiny-G").move_xyza(Some(1.0), Some(-1.0), None, None); });
    builder.get_object::<gtk::Button>("x_plus_y_plus_button").unwrap().connect_clicked(|_button| { TINY_G.lock().expect("Unable to lock Tiny-G").move_xyza(Some(1.0), Some(1.0), None, None); });
    builder.get_object::<gtk::Button>("z_minus_button").unwrap().connect_clicked(|_button| { TINY_G.lock().expect("Unable to lock Tiny-G").move_xyza(None, None, Some(-1.0), None); });
    builder.get_object::<gtk::Button>("z_plus_button").unwrap().connect_clicked(|_button| { TINY_G.lock().expect("Unable to lock Tiny-G").move_xyza(None, None, Some(1.0), None); });

    let pos_x: gtk::Label = builder.get_object("pos_x").unwrap();
    let pos_y: gtk::Label = builder.get_object("pos_y").unwrap();
    let pos_z: gtk::Label = builder.get_object("pos_z").unwrap();
    let pos_a: gtk::Label = builder.get_object("pos_a").unwrap();

    let (sender, receiver) = glib::MainContext::channel(glib::PRIORITY_DEFAULT);

    thread::spawn(move || {
        loop {
            thread::sleep(time::Duration::from_millis(100));
            // Sending fails if the receiver is closed
            let status= TINY_G.lock().expect("Unable to lock Tiny-G").get_latest_status().unwrap();
            let _ = sender.send(Message::UpdateLabel(status));
        }
    });

// Attach the receiver to the default main context (None)
// and on every message update the label accordingly.
    let pos_x_clone = pos_x.clone();
    let pos_y_clone = pos_y.clone();
    let pos_z_clone = pos_z.clone();
    let pos_a_clone = pos_a.clone();
    receiver.attach(None, move |msg| {
        match msg {
            Message::UpdateLabel(status) => {
                pos_x_clone.set_text(format!("{:.4}", status.posx).as_str());
                pos_y_clone.set_text(format!("{:.4}", status.posy).as_str());
                pos_z_clone.set_text(format!("{:.4}", status.posz).as_str());
                pos_a_clone.set_text(format!("{:.4}", status.posa).as_str());
            },
        }
        // Returning false here would close the receiver
        // and have senders fail
        glib::Continue(true)
    });

    main_window.connect_delete_event(|_, _| {
        // Stop the main loop.
        gtk::main_quit();
        // Let the default handler destroy the window.
        Inhibit(false)
    });

    main_window.show_all();

    gtk::main();
}
