use std::process::exit;
use crate::tinyg::{Tinyg};
use crate::whb04b::*;
use std::io::{BufReader, Read, BufRead};
use std::io;

extern crate gdk;
extern crate gtk;
extern crate gio;

use gtk::prelude::*;
use glib::{clone, idle_add_local};

use std::fs::File;
use std::thread;
use std::path::Path;
use std::str::FromStr;
use std::sync::{Mutex};

use lazy_static::lazy_static;
use gtk::TextTagBuilder;

use log::{error, info};
use simple_logger::SimpleLogger;
use log::LevelFilter::Info;

mod tinyg;
mod whb04b;

enum Message {
    UpdatePosition(tinyg::Status),
    UpdateLine(tinyg::Status),
    UpdateQueueFree(tinyg::QueueStatus)
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

fn get_selected_distance(builder:gtk::Builder) -> f32 {
    let full_button: gtk::RadioButton = builder.object("move_full_radio").unwrap();
    let tenth_button: gtk::RadioButton = builder.object("move_tenth_radio").unwrap();
    let hundreds_button: gtk::RadioButton = builder.object("move_hundreds_radio").unwrap();
    let thousands_button: gtk::RadioButton = builder.object("move_thousands_radio").unwrap();
    if full_button.is_active() {
        1 as f32
    }
    else if tenth_button.is_active() {
        0.1 as f32
    }
    else if hundreds_button.is_active() {
        0.01 as f32
    }
    else if thousands_button.is_active() {
        0.001 as f32
    }
    else {
        let distance_input: gtk::Entry = builder.object("distance_entry").unwrap();
        let text = distance_input.text();
        let str = text.as_str();
        match <f32 as FromStr>::from_str(str) {
            Ok(value) => {
                value
            }
            Err(_err) => 0.0
        }
    }
}

fn get_selected_rpm(builder:gtk::Builder) -> i32 {
    let rpm_input: gtk::Entry = builder.object("rpm_entry").unwrap();
    let text = rpm_input.text();
    let str = text.as_str();
    <i32 as FromStr>::from_str(str).unwrap()
}

pub fn main() {
    SimpleLogger::new().with_level(Info).without_timestamps().init().unwrap();

    let comm_thread;
    match TINY_G.lock().expect("Unable to lock Tiny-G").initialize() {
        Ok(ct) => {
            info!("Initialization complete.");
            comm_thread = ct;
        }
        Err(error) => {
            error!("Error: {}", error);
            exit(0);
        }
    }

    match TINY_G.lock().expect("Unable to lock Tiny-G").get_system_status() {
        Ok(_result) => {
        }
        Err(error) => {
            error!("Error: {}", error);
            exit(0);
        }
    }

    if let Ok(lines) = read_lines("./config") {
        for line in lines {
            if let Ok(cfg) = line {
                match TINY_G.lock().expect("Unable to lock Tiny-G").send_config(cfg)
                {
                    Ok(_result) => {
                    }
                    Err(error) => {
                        error!("Error: {}", error);
                        exit(0);
                    }
                }
            }
        }
    }

    match TINY_G.lock().expect("Unable to lock Tiny-G").set_status_fields() {
        Ok(_result) => {
        }
        Err(error) => {
            error!("Error: {}", error);
            exit(0);
        }
    }

    match TINY_G.lock().expect("Unable to lock Tiny-G").get_status() {
        Ok(_result) => {
        }
        Err(error) => {
            error!("Error: {}", error);
            exit(0);
        }
    }

    let whb_thread = Whb04b::initialize(|| TINY_G.lock().expect("Unable to lock Tiny-G").clone());

    if gtk::init().is_err() {
        error!("Failed to initialize GTK.");
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
        &gdk::Screen::default().expect("Error initializing gtk css provider."),
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let glade_src = include_str!("../mainwindow.glade");
    let builder = gtk::Builder::from_string(glade_src);

    let main_window: gtk::Window = builder.object("main_window").unwrap();
    let text_view: gtk::TextView = builder.object("gcode_view").unwrap();

    let status_label : gtk::Label = builder.object("status").unwrap();

    let file_choose_button : gtk::FileChooserButton = builder.object("file_choose_button").unwrap();
    file_choose_button.connect_file_set(clone!(@weak text_view => move |file_choose_button| {
        let filename = file_choose_button.filename().expect("Couldn't get filename");
        let file = File::open(&filename).expect("Couldn't open file");

        let mut reader = BufReader::new(file);
        let mut contents = String::new();
        let _ = reader.read_to_string(&mut contents);

        text_view
            .buffer()
            .expect("Couldn't get window")
            .set_text(&contents);

        let mut gcode_lines : Vec<String> = Vec::new();
        let mut line_number = 0;
        contents.lines().for_each(
            |x| {
                let mut s = String::new();
                s.push_str(format!("N{:05} ", line_number).as_str());
                s.push_str(x.splitn(2, ';').next().unwrap().trim());
                gcode_lines.push(s);
                line_number += 1;
            }
        );

        thread::spawn(move || {
            let ting_ref = TINY_G.lock().expect("Unable to lock Tiny-G");
            let mut tinyg = ting_ref.clone();
            drop(ting_ref);

            tinyg.send_gcode(Box::new(gcode_lines));
        });
    }));

    builder.object::<gtk::Button>("refallhome_button").unwrap().connect_clicked(|_button| {
        match TINY_G.lock().expect("Unable to lock Tiny-G").home_all() {
            Ok(_) => {

            },
            Err(msg) => {
                error!("Ref All Home: {}", msg);
            }
        }
    });

    builder.object::<gtk::Button>("zerox_button").unwrap().connect_clicked(|_button| {
        match TINY_G.lock().expect("Unable to lock Tiny-G").zero_x() {
            Ok(_) => {

            },
            Err(msg) => {
                error!("Zero X: {}", msg);
            }
        }
    });

    builder.object::<gtk::Button>("zeroy_button").unwrap().connect_clicked(|_button| {
        match TINY_G.lock().expect("Unable to lock Tiny-G").zero_y() {
            Ok(_) => {

            },
            Err(msg) => {
                error!("Zero Y: {}", msg);
            }
        }
    });

    builder.object::<gtk::Button>("zeroz_button").unwrap().connect_clicked(|_button| {
        match TINY_G.lock().expect("Unable to lock Tiny-G").zero_z() {
            Ok(_) => {

            },
            Err(msg) => {
                error!("Zero Z: {}", msg);
            }
        }
    });

    builder.object::<gtk::Button>("zeroa_button").unwrap().connect_clicked(|_button| {
        match TINY_G.lock().expect("Unable to lock Tiny-G").zero_a() {
            Ok(_) => {

            },
            Err(msg) => {
                error!("Zero A: {}", msg);
            }
        }
    });

    builder.object::<gtk::Button>("cycle_start_button").unwrap().connect_clicked(|_button| {
        TINY_G2.lock().expect("Unable to lock Tiny-G").cycle_start();
    });

    builder.object::<gtk::Button>("feed_hold_button").unwrap().connect_clicked(|_button| {
        TINY_G2.lock().expect("Unable to lock Tiny-G").feed_hold();
    });

    builder.object::<gtk::Button>("x_minus_button").unwrap().connect_clicked(clone!(@weak builder => move |_button| {
        let distance = get_selected_distance(builder);
        match TINY_G.lock().expect("Unable to lock Tiny-G").move_xyza(Some(-distance), None, None, None) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("X Minus: {}", msg);
            }
        }
    }));

    builder.object::<gtk::Button>("x_plus_button").unwrap().connect_clicked(clone!(@weak builder => move |_button| {
        let distance = get_selected_distance(builder);
        match TINY_G.lock().expect("Unable to lock Tiny-G").move_xyza(Some(distance), None, None, None) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("X Plus: {}", msg);
            }
        }
    }));

    builder.object::<gtk::Button>("y_minus_button").unwrap().connect_clicked(clone!(@weak builder => move |_button| {
        let distance = get_selected_distance(builder);
        match TINY_G.lock().expect("Unable to lock Tiny-G").move_xyza(None, Some(-distance), None, None) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("Y Minus: {}", msg);
            }
        }
    }));

    builder.object::<gtk::Button>("y_plus_button").unwrap().connect_clicked(clone!(@weak builder => move |_button| {
        let distance = get_selected_distance(builder);
        match TINY_G.lock().expect("Unable to lock Tiny-G").move_xyza(None, Some(distance), None, None) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("Y Plus: {}", msg);
            }
        }
    }));

    builder.object::<gtk::Button>("x_minus_y_minus_button").unwrap().connect_clicked(clone!(@weak builder => move |_button| {
        let distance = get_selected_distance(builder);
        match TINY_G.lock().expect("Unable to lock Tiny-G").move_xyza(Some(-distance), Some(-distance), None, None) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("X Minus Y Minus: {}", msg);
            }
        }
    }));

    builder.object::<gtk::Button>("x_minus_y_plus_button").unwrap().connect_clicked(clone!(@weak builder => move |_button| {
        let distance = get_selected_distance(builder);
        match TINY_G.lock().expect("Unable to lock Tiny-G").move_xyza(Some(-distance), Some(distance), None, None) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("X Minus Y Plus: {}", msg);
            }
        }
    }));

    builder.object::<gtk::Button>("x_plus_y_minus_button").unwrap().connect_clicked(clone!(@weak builder => move |_button| {
        let distance = get_selected_distance(builder);
        match TINY_G.lock().expect("Unable to lock Tiny-G").move_xyza(Some(distance), Some(-distance), None, None) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("X Plus Y Minus: {}", msg);
            }
        }
    }));

    builder.object::<gtk::Button>("x_plus_y_plus_button").unwrap().connect_clicked(clone!(@weak builder => move |_button| {
        let distance = get_selected_distance(builder);
        match TINY_G.lock().expect("Unable to lock Tiny-G").move_xyza(Some(distance), Some(distance), None, None) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("X Plus Y Plus: {}", msg);
            }
        }
    }));

    builder.object::<gtk::Button>("z_minus_button").unwrap().connect_clicked(clone!(@weak builder => move |_button| {
        let distance = get_selected_distance(builder);
        match TINY_G.lock().expect("Unable to lock Tiny-G").move_xyza(None, None, Some(-distance), None) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("Z Minus: {}", msg);
            }
        }
    }));

    builder.object::<gtk::Button>("z_plus_button").unwrap().connect_clicked(clone!(@weak builder => move |_button| {
        let distance = get_selected_distance(builder);
        match TINY_G.lock().expect("Unable to lock Tiny-G").move_xyza(None, None, Some(distance), None) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("Z Plus: {}", msg);
            }
        }
    }));

    builder.object::<gtk::Button>("spindle_cw_button").unwrap().connect_clicked(clone!(@weak builder => move |_button| {
        let speed = get_selected_rpm(builder);
        match TINY_G.lock().expect("Unable to lock Tiny-G").spindle_cw(speed) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("Spindle CW: {}", msg);
            }
        }
    }));

    builder.object::<gtk::Button>("spindle_ccw_button").unwrap().connect_clicked(clone!(@weak builder => move |_button| {
        let speed = get_selected_rpm(builder);
        match TINY_G.lock().expect("Unable to lock Tiny-G").spindle_ccw(speed) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("Spindle CW: {}", msg);
            }
        }
    }));

    builder.object::<gtk::Button>("spindle_stop_button").unwrap().connect_clicked(|_| {
        match TINY_G.lock().expect("Unable to lock Tiny-G").spindle_stop() {
            Ok(_) => {

            },
            Err(msg) => {
                error!("Spindle Stop: {}", msg);
            }
        }
    });

    let pos_x: gtk::Label = builder.object("pos_x").unwrap();
    let pos_y: gtk::Label = builder.object("pos_y").unwrap();
    let pos_z: gtk::Label = builder.object("pos_z").unwrap();
    let pos_a: gtk::Label = builder.object("pos_a").unwrap();

    let (sender, receiver) = glib::MainContext::channel(glib::PRIORITY_HIGH);

    let mut tiny_g = TINY_G.lock().expect("Unable to lock Tiny-G");
    let mut old_status = tiny_g.get_latest_status().unwrap();
    let mut old_queue_status = tiny_g.get_queue_status();
    drop(tiny_g);
    let _ = sender.send(Message::UpdatePosition(old_status));
    let _ = sender.send( Message::UpdateQueueFree(old_queue_status));

    idle_add_local(move || {
        let mut tiny_g = TINY_G.lock().expect("Unable to lock Tiny-G");
        let status= tiny_g.get_latest_status().unwrap();
        let queue_status = tiny_g.get_queue_status();
        drop(tiny_g);
        if old_status != status {
            if old_status.line != status.line
            {
                let _ = sender.send(Message::UpdateLine(status));
            }
            if old_status.posx != status.posx || old_status.posy != status.posy || old_status.posz != status.posz || old_status.posa != status.posa
            {
                let _ = sender.send(Message::UpdatePosition(status));
            }
            old_status = status;
        }
        if old_queue_status != queue_status {
            old_queue_status = queue_status;
            let _ = sender.send( Message::UpdateQueueFree(queue_status));
        }
        Continue(true)
    });

    let pos_x_clone = pos_x.clone();
    let pos_y_clone = pos_y.clone();
    let pos_z_clone = pos_z.clone();
    let pos_a_clone = pos_a.clone();
    let text_view_clone = text_view.clone();
    let status_label_clone = status_label.clone();
    let text_view_buffer = text_view_clone.buffer().unwrap();

    let tag = TextTagBuilder::new().background("yellow").name("yellow_bg").build();
    text_view_buffer.tag_table().unwrap().add(&tag);

    receiver.attach(None, move |msg| {
        match msg {
            Message::UpdatePosition(status) => {
                pos_x_clone.set_text(format!("{:.4}", status.posx).as_str());
                pos_y_clone.set_text(format!("{:.4}", status.posy).as_str());
                pos_z_clone.set_text(format!("{:.4}", status.posz).as_str());
                pos_a_clone.set_text(format!("{:.4}", status.posa).as_str());
            }
            Message::UpdateLine(status) => {
                let iter = text_view_buffer.iter_at_line(status.line as i32 + 5);
                match text_view_buffer.create_mark(None, &iter, false) {
                    Some(mark) => {
                        text_view_clone.scroll_mark_onscreen(&mark);
                        text_view_buffer.delete_mark(&mark);
                    }
                    None => {
                    }
                }

                text_view_buffer.remove_tag(&tag, &text_view_buffer.start_iter(), &text_view_buffer.end_iter());
                let iter = text_view_buffer.iter_at_line(status.line as i32);
                text_view_buffer.apply_tag(&tag, &iter, &text_view_buffer.iter_at_line(status.line as i32 + 1));
            },
            Message::UpdateQueueFree(queue_status) => {
                status_label_clone.set_text(format!("Free planning queue entries: {:>2}, Lines read and ready to be consumed: {:>2}, Input buffer length: {:>4}", queue_status.tinyg_planning_buffer_free, queue_status.line_buffer_length, queue_status.input_buffer_length).as_str());
            }
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
    let _ = whb_thread.1.send(());
    whb_thread.0.join().unwrap();
    let _ = comm_thread.1.send(());
    comm_thread.0.join().unwrap();
}
