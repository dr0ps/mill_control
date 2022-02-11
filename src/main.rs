use std::process::exit;
use crate::tinyg::{Tinyg};
use crate::whb04b::*;
use crate::g_render::*;

use std::io::{BufReader, Read, BufRead};
use std::io;

extern crate gdk;
extern crate gtk;
extern crate gio;

use gio::prelude::*;

use glib::{clone, idle_add_local};
use glib::signal::Inhibit;

use gtk::prelude::*;

use std::fs::File;
use std::thread;
use std::path::Path;
use std::str::FromStr;
use std::sync::{Mutex};
use std::time::Duration;
use gdk::{EventMask};
use gdk::pango::Style;

use lazy_static::lazy_static;

use log::{error, info, warn};
use simple_logger::SimpleLogger;
use log::LevelFilter::Info;

mod tinyg;
mod whb04b;
mod vertex;
mod gl_area_backend;
mod gl_facade;
mod g_render;
mod stylus;

enum Message {
    UpdatePosition(tinyg::Status),
    UpdateLine(tinyg::Status),
    UpdateCoordinateSystem(tinyg::Status),
    UpdateQueueFree(tinyg::QueueStatus),
    ProgrammStarted(),
    ProgrammStopped()
}

lazy_static! {
    static ref TINY_G : Mutex<Tinyg> = Mutex::new(Tinyg::new());
    static ref TINY_G2 : Mutex<Tinyg> = Mutex::new(TINY_G.lock().unwrap().clone());
    static ref G_RENDER : Mutex<GRender> = Mutex::new(GRender::new());
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

    let current_line_tag = gtk::TextTagBuilder::new().background("yellow").name("yellow_bg").build();
    let disabled_line_tag = gtk::TextTagBuilder::new().foreground("grey").style(Style::Italic).name("grey_fg").build();

    let start_mark;
    {
        let text_view_buffer = text_view.buffer().expect("Couldn't get buffer");
        text_view_buffer.tag_table().unwrap().add(&current_line_tag);
        text_view_buffer.tag_table().unwrap().add(&disabled_line_tag);
        start_mark = text_view_buffer.create_mark(Some("Start"), &text_view_buffer.start_iter(), true).expect("Unable to create mark.");
    }
    start_mark.set_visible(true);

    let status_label : gtk::Label = builder.object("status").unwrap();

    let gl_area_event_box: gtk::EventBox = builder.object("gl_area_event_box").unwrap();
    let gl_area: gtk::GLArea = builder.object("gl_area").unwrap();
    gl_area.add_events(EventMask::BUTTON_PRESS_MASK | EventMask::BUTTON_RELEASE_MASK | EventMask::POINTER_MOTION_MASK | EventMask::SCROLL_MASK | EventMask::SMOOTH_SCROLL_MASK);
    gl_area.connect_motion_notify_event(|_gl_area, event_motion| {
        let pos = event_motion.position();
        G_RENDER.lock().expect("Unable to lock G_RENDER").set_angle(pos.0 as f32, pos.1 as f32);
        Inhibit(true)
    });
    gl_area.connect_scroll_event(|_gl_area, event_scroll| {
        let (_x, y) = event_scroll.scroll_deltas().unwrap();
        G_RENDER.lock().expect("Unable to lock G_RENDER").set_zoom(y as f32 / 10.0);
        Inhibit(true)
    });
    gl_area_event_box.connect_enter_notify_event(|_gl_area, _event_crossing| {
        G_RENDER.lock().expect("Unable to lock G_RENDER").disable_auto_reset_rotation();
        Inhibit(true)
    });
    gl_area_event_box.connect_leave_notify_event(|_gl_area, _event_crossing| {
        G_RENDER.lock().expect("Unable to lock G_RENDER").enable_auto_reset_rotation();
        Inhibit(true)
    });

    let file_choose_button : gtk::FileChooserButton = builder.object("file_choose_button").unwrap();
    file_choose_button.connect_file_set(clone!(@weak text_view, @weak start_mark => move |file_choose_button| {
        let filename = file_choose_button.filename().expect("Couldn't get filename");
        let file = File::open(&filename).expect("Couldn't open file");

        let mut reader = BufReader::new(file);
        let mut contents = String::new();
        let _ = reader.read_to_string(&mut contents);

        let text_view_buffer = text_view
            .buffer()
            .expect("Couldn't get buffer");

        text_view_buffer.set_text(&contents);

        text_view_buffer.move_mark(&start_mark, &text_view_buffer.start_iter());

        match G_RENDER.lock().expect("").update(&contents) {
            Err(error) => {
                error!("Error in update {}", error);
            }
            _ => {}
        }
    }));

    let jog_box : gtk::Box = builder.object("box_jog").unwrap();
    let spindle_box : gtk::Box = builder.object("box_spindle").unwrap();
    let position_box : gtk::Box = builder.object("box_position").unwrap();

    let start_button : gtk::Button = builder.object::<gtk::Button>("start_button").unwrap();
    start_button.connect_clicked(clone!(@weak text_view, @weak start_mark => move |_button| {
        let buffer = text_view
            .buffer()
            .expect("Couldn't get buffer");

        let (start, end) = (buffer.iter_at_mark(&start_mark), buffer.end_iter());

        let contents = buffer.text(&start, &end, true).expect("Couldn't get contents.");

        let mut gcode_lines : Vec<String> = Vec::new();
        let mut line_number = buffer.iter_at_mark(&start_mark).line();
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

    let start_line_button : gtk::Button = builder.object::<gtk::Button>("start_line_button").unwrap();
    start_line_button.connect_clicked(clone!(@weak text_view, @weak start_mark => move |_button| {
        let (strong, _weak) = text_view
            .cursor_locations(None);

        let buffer = text_view
            .buffer()
            .expect("Couldn't get buffer");

        let line = text_view.iter_at_location(strong.x, strong.y).unwrap().line();

        buffer.remove_tag(&disabled_line_tag, &buffer.start_iter(), &buffer.end_iter());
        let iter = buffer.start_iter();
        buffer.apply_tag(&disabled_line_tag, &iter, &buffer.iter_at_line(line));

        buffer.move_mark(&start_mark, &buffer.iter_at_line(line));

         G_RENDER.lock().expect("").set_start_line(line as u32);
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

    builder.object::<gtk::Button>("reset_button").unwrap().connect_clicked(|_button| {
        TINY_G2.lock().expect("Unable to lock Tiny-G").reset();
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

    main_window.show_all();

    gl_loader::init_gl();

    let (facade, program) = G_RENDER.lock().expect("Unable to lock G_RENDER").initialize(&gl_area);

    gl_area.connect_render(move |_glarea, _glcontext| {
        G_RENDER.lock().expect("Unable to lock G_RENDER").draw(&facade, &program);
        Inhibit(true)
    });

    gl_area.connect_resize(move |_gl_area, width,height| {
        G_RENDER.lock().expect("Unable to lock G_RENDER").resize(width, height);
    });

    const FPS: u64 = 60;
    glib::source::timeout_add_local(Duration::from_millis(1_000 / FPS), move || {
        gl_area.queue_draw();
        glib::source::Continue(true)
    });

    let (sender, receiver) = glib::MainContext::channel(glib::PRIORITY_HIGH);

    let mut tiny_g = TINY_G.lock().expect("Unable to lock Tiny-G");
    let mut old_status = tiny_g.get_latest_status().unwrap();
    old_status.stat = 1;
    let mut old_queue_status = tiny_g.get_queue_status();
    drop(tiny_g);
    let _ = sender.send(Message::UpdatePosition(old_status));
    let _ = sender.send( Message::UpdateQueueFree(old_queue_status));
    let _ = sender.send(Message::UpdateCoordinateSystem(old_status));

    idle_add_local(move || {
        let mut tiny_g = TINY_G.lock().expect("Unable to lock Tiny-G");
        let status= tiny_g.get_latest_status().unwrap();
        let queue_status = tiny_g.get_queue_status();
        drop(tiny_g);
        if old_status != status {
            if old_status.line != status.line {
                let _ = sender.send(Message::UpdateLine(status));
                G_RENDER.lock().expect("Unable to lock G_RENDER").update_line(status.line);
            }
            if old_status.posx != status.posx || old_status.posy != status.posy || old_status.posz != status.posz || old_status.posa != status.posa {
                G_RENDER.lock().expect("Unable to lock G_RENDER").set_position(status.posx, status.posy, status.posz);
                let _ = sender.send(Message::UpdatePosition(status));
            }
            if old_status.coor != status.coor {
                let _ = sender.send(Message::UpdateCoordinateSystem(status));
            }
            if old_status.stat != status.stat {
                match status.stat {
                    1 | 4 => {
                        let _ = sender.send(Message::ProgrammStopped());
                    }
                    _ => {
                        let _ = sender.send(Message::ProgrammStarted());
                    }
                }
            }
            old_status = status;
        }
        if old_queue_status != queue_status {
            old_queue_status = queue_status;
            let _ = sender.send( Message::UpdateQueueFree(queue_status));
        }
        Continue(true)
    });

    let g54 : gtk::RadioButton = builder.object("g54").unwrap();
    g54.connect_clicked(|_| {
        match TINY_G.lock().expect("Unable to lock Tiny-G").set_coordinate_sytem(1) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("G54: {}", msg);
            }
        }
    });
    let g55 : gtk::RadioButton = builder.object("g55").unwrap();
    g55.connect_clicked(|_| {
        match TINY_G.lock().expect("Unable to lock Tiny-G").set_coordinate_sytem(2) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("G55: {}", msg);
            }
        }
    });
    let g56 : gtk::RadioButton = builder.object("g56").unwrap();
    g56.connect_clicked(|_| {
        match TINY_G.lock().expect("Unable to lock Tiny-G").set_coordinate_sytem(3) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("G56: {}", msg);
            }
        }
    });
    let g57 : gtk::RadioButton = builder.object("g57").unwrap();
    g57.connect_clicked(|_| {
        match TINY_G.lock().expect("Unable to lock Tiny-G").set_coordinate_sytem(4) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("G57: {}", msg);
            }
        }
    });
    let g58 : gtk::RadioButton = builder.object("g58").unwrap();
    g58.connect_clicked(|_| {
        match TINY_G.lock().expect("Unable to lock Tiny-G").set_coordinate_sytem(5) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("G58: {}", msg);
            }
        }
    });
    let g59 : gtk::RadioButton = builder.object("g59").unwrap();
    g59.connect_clicked(|_| {
        match TINY_G.lock().expect("Unable to lock Tiny-G").set_coordinate_sytem(6) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("G59: {}", msg);
            }
        }
    });

    receiver.attach(None, move |msg| {
        match msg {
            Message::UpdatePosition(status) => {
                pos_x.set_text(format!("{:.4}", status.posx).as_str());
                pos_y.set_text(format!("{:.4}", status.posy).as_str());
                pos_z.set_text(format!("{:.4}", status.posz).as_str());
                pos_a.set_text(format!("{:.4}", status.posa).as_str());
            }
            Message::UpdateLine(status) => {
                let buffer = text_view
                    .buffer()
                    .expect("Couldn't get buffer");
                let iter = buffer.iter_at_line(status.line as i32 + 5);
                match buffer.create_mark(None, &iter, false) {
                    Some(mark) => {
                        text_view.scroll_mark_onscreen(&mark);
                        buffer.delete_mark(&mark);
                    }
                    None => {
                    }
                }

                buffer.remove_tag(&current_line_tag, &buffer.start_iter(), &buffer.end_iter());
                let iter = buffer.iter_at_line(status.line as i32);
                buffer.apply_tag(&current_line_tag, &iter, &buffer.iter_at_line(status.line as i32 + 1));
            },
            Message::UpdateQueueFree(queue_status) => {
                status_label.set_text(format!("Free planning queue entries: {:>2}, Lines read and ready to be consumed: {:>2}, Input buffer length: {:>4}", queue_status.tinyg_planning_buffer_free, queue_status.line_buffer_length, queue_status.input_buffer_length).as_str());
            },
            Message::UpdateCoordinateSystem(status) => {
                match status.coor {
                    1 => g54.activate(),
                    2 => g55.activate(),
                    3 => g56.activate(),
                    4 => g57.activate(),
                    5 => g58.activate(),
                    6 => g59.activate(),
                    _ => {
                        warn!("Unsupported coordinate system {}", status.coor);
                        true
                    }
                };
            },
            Message::ProgrammStarted() => {
                file_choose_button.set_sensitive(false);
                start_button.set_sensitive(false);
                start_line_button.set_sensitive(false);
                jog_box.set_sensitive(false);
                spindle_box.set_sensitive(false);
                position_box.set_sensitive(false);
            }
            Message::ProgrammStopped() => {
                file_choose_button.set_sensitive(true);
                start_button.set_sensitive(true);
                start_line_button.set_sensitive(true);
                jog_box.set_sensitive(true);
                spindle_box.set_sensitive(true);
                position_box.set_sensitive(true);
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

    gtk::main();
    let _ = whb_thread.1.send(());
    whb_thread.0.join().unwrap();
    let _ = comm_thread.1.send(());
    comm_thread.0.join().unwrap();
}
