use std::process::exit;
use crate::tinyg::{Tinyg};
use std::io::{stdout, Write, BufReader, Read};

extern crate gdk;
extern crate gtk;
extern crate gio;

use gtk::prelude::*;
use glib::{clone};
use std::fs::File;

use std::thread;
use std::time;

mod tinyg;

enum Message {
    UpdateLabel(tinyg::Status),
}

pub fn main() {
    let mut tinyg = Tinyg::new();
    match tinyg.initialize() {
        Ok(()) => {
            println!("Initialization complete.");
            stdout().flush().unwrap();
        }
        Err(error) => {
            println!("Error: {}", error);
            exit(0);
        }
    }
    match tinyg.get_system_status() {
        Ok(result) => {
            println!("Status: {}", result);
            stdout().flush().unwrap();
        }
        Err(error) => {
            println!("Error: {}", error);
            exit(0);
        }
    }
    match tinyg.get_status() {
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

    }));

    let pos_x: gtk::Label = builder.get_object("pos_x").unwrap();
    let pos_y: gtk::Label = builder.get_object("pos_y").unwrap();
    let pos_z: gtk::Label = builder.get_object("pos_z").unwrap();
    let pos_a: gtk::Label = builder.get_object("pos_a").unwrap();

    let (sender, receiver) = glib::MainContext::channel(glib::PRIORITY_DEFAULT);
    let button : gtk::Button = builder.get_object("refallhome_button").unwrap();
    button.connect_clicked(clone!(@strong tinyg => move |_x| { tinyg.clone().home_all(); }));

    thread::spawn(move || {
        loop {
            thread::sleep(time::Duration::from_millis(100));
            // Sending fails if the receiver is closed
            let status= tinyg.get_latest_status().unwrap();
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
