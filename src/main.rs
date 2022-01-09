use std::process::exit;
use crate::tinyg::{Tinyg};
use crate::whb04b::*;
use crate::g_render::*;

use std::io::{BufReader, Read, BufRead};
use std::io;

extern crate gdk4;
extern crate gtk4;
extern crate gio;

use gio::prelude::*;

use glib::{clone, idle_add_local};
use glib::signal::Inhibit;

use gtk4::prelude::*;

use std::fs::File;
use std::thread;
use std::path::Path;
use std::str::FromStr;
use std::sync::{Mutex};
use std::time::Duration;
use gdk4::pango::Style;
use gtk4::{EventControllerMotionBuilder, EventControllerScrollBuilder, EventControllerScrollFlags};

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

fn get_selected_distance(full_button: gtk4::ToggleButton,
                         tenth_button: gtk4::ToggleButton,
                         hundreds_button: gtk4::ToggleButton,
                         thousands_button: gtk4::ToggleButton,
                         distance_input: gtk4::Entry) -> f32 {
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

fn get_selected_rpm(rpm_input: gtk4::Entry) -> i32 {
    let text = rpm_input.text();
    let str = text.as_str();
    <i32 as FromStr>::from_str(str).unwrap()
}

pub fn main() {
    SimpleLogger::new().with_level(Info).without_timestamps().init().unwrap();

    let comm_sender;
    match TINY_G.lock().expect("Unable to lock Tiny-G").initialize() {
        Ok(ct) => {
            info!("Initialization complete.");
            comm_sender = ct.1;
        }
        Err(error) => {
            error!("Error: {}", error);
            exit(0);
        }
    }

    match TINY_G.lock().expect("Unable to lock Tiny-G").get_system_status() {
        Ok(_result) => {}
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
                    Ok(_result) => {}
                    Err(error) => {
                        error!("Error: {}", error);
                        exit(0);
                    }
                }
            }
        }
    }

    match TINY_G.lock().expect("Unable to lock Tiny-G").set_status_fields() {
        Ok(_result) => {}
        Err(error) => {
            error!("Error: {}", error);
            exit(0);
        }
    }

    match TINY_G.lock().expect("Unable to lock Tiny-G").get_status() {
        Ok(_result) => {}
        Err(error) => {
            error!("Error: {}", error);
            exit(0);
        }
    }

    let whb_sender = Whb04b::initialize(|| TINY_G.lock().expect("Unable to lock Tiny-G").clone()).1;

    let application = gtk4::Application::new(Some("com.github.dr0ps.mill_control"), Default::default());
    application.connect_activate(build_ui);
    application.connect_shutdown(move |_| {
        match whb_sender.send(()) {
            Err(send_error) => {
                warn!("Unable to send stop signal to whb handler, maybe already closed? {}", send_error)
            },
            _ => {}
        }
        comm_sender.send(()).expect("Unable to send into comm thread.");
    });
    application.run();
}

pub fn build_ui(application: &gtk4::Application) {
    if gtk4::init().is_err() {
        error!("Failed to initialize GTK.");
        return;
    }

    let style = include_str!("../style.css");
    let provider = gtk4::CssProvider::new();
    provider
        .load_from_data(style.as_bytes());
    // We give the CssProvided to the default screen so the CSS rules we added
    // can be applied to our window.
    gtk4::StyleContext::add_provider_for_display(
        &gdk4::Display::default().expect("Error initializing gtk css provider."),
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );

    let glade_src = include_str!("../mainwindow.xml");
    let builder = gtk4::Builder::from_string(glade_src);

    let main_window: gtk4::ApplicationWindow = builder.object("main_window").unwrap();
    main_window.set_application(Some(application));

    let current_line_tag = gtk4::TextTagBuilder::new().background("yellow").name("yellow_bg").build();
    let disabled_line_tag = gtk4::TextTagBuilder::new().foreground("grey").style(Style::Italic).name("grey_fg").build();

    let text_view: gtk4::TextView = builder.object("gcode_view").unwrap();

    let start_mark;
    {
        let text_view_buffer = text_view.buffer();
        text_view_buffer.tag_table().add(&current_line_tag);
        text_view_buffer.tag_table().add(&disabled_line_tag);
        start_mark = text_view_buffer.create_mark(Some("Start"), &text_view_buffer.start_iter(), true);
    }
    start_mark.set_visible(true);

    let status_label : gtk4::Label = builder.object("status").unwrap();

    let gl_area: gtk4::GLArea = builder.object("gl_area").unwrap();
    let motion = EventControllerMotionBuilder::new().build();
    motion.connect_motion(|_, x, y| {
        G_RENDER.lock().expect("Unable to lock G_RENDER").set_angle(x as f32, y as f32);
    });
    motion.connect_enter(|_, _, _| {
        G_RENDER.lock().expect("Unable to lock G_RENDER").disable_auto_reset_rotation();
    });
    motion.connect_leave(|_| {
        G_RENDER.lock().expect("Unable to lock G_RENDER").enable_auto_reset_rotation();
    });
    let scroll = EventControllerScrollBuilder::new().build();
    scroll.set_flags(EventControllerScrollFlags::VERTICAL);
    scroll.connect_scroll(|_, _x, y| {
        G_RENDER.lock().expect("Unable to lock G_RENDER").set_zoom(y as f32 / 10.0);
        Inhibit(true)
    });

    gl_area.add_controller(&motion);
    gl_area.add_controller(&scroll);

    let filter : gtk4::FileFilter = builder.object("gcode_file_filter").unwrap();
    let file_choose_button : gtk4::Button = builder.object("file_chooser_button").unwrap();
    file_choose_button.connect_clicked(clone!(@weak text_view, @weak main_window, @weak start_mark => move |_button| {
        let dialog = gtk4::FileChooserDialog::new(
            Some("Open File"),
            Some(&main_window),
            gtk4::FileChooserAction::Open,
            &[("Open", gtk4::ResponseType::Ok), ("Cancel", gtk4::ResponseType::Cancel)]
        );
        dialog.set_modal(true);
        dialog.add_filter(&filter);
        dialog.connect_response(clone!(@weak start_mark => move |dialog : &gtk4::FileChooserDialog, response: gtk4::ResponseType| {
            if response == gtk4::ResponseType::Ok {
                let gio_file = dialog.file().expect("Couldn't get file.");
                let filename = gio_file.path().expect("Couldn't get file path");
                let file = File::open(&filename).expect("Couldn't open file");

                let mut reader = BufReader::new(file);
                let mut contents = String::new();
                let _ = reader.read_to_string(&mut contents);

                text_view
                    .buffer()
                    .set_text(&contents);

                text_view.buffer().move_mark(&start_mark, &text_view.buffer().start_iter());

                match G_RENDER.lock().expect("").update(&contents) {
                    Err(error) => {
                        error!("Error in update {}", error);
                    }
                    _ => {}
                }
            }
            dialog.close();
        }));
        dialog.show();
    }));

    let jog_box : gtk4::Box = builder.object("box_jog").unwrap();
    let spindle_box : gtk4::Box = builder.object("box_spindle").unwrap();
    let position_box : gtk4::Box = builder.object("box_position").unwrap();

    let start_button : gtk4::Button = builder.object::<gtk4::Button>("start_button").unwrap();
    start_button.connect_clicked(clone!(@weak text_view, @weak start_mark => move |_button| {
        let buffer = text_view
            .buffer();

        let (start, end) = (buffer.iter_at_mark(&start_mark), buffer.end_iter());

        let contents = buffer.text(&start, &end, true);

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

            match tinyg.send_gcode(Box::new(gcode_lines)) {
                Err(msg) => warn!("Error when sending gcode: {}", msg),
                _ => {}
            }
        });
    }));

    let stop_button : gtk4::Button = builder.object::<gtk4::Button>("stop_button").unwrap();
    stop_button.connect_clicked(move |_button| {
        let mut tinyg_ref = TINY_G.lock().expect("Unable to lock Tiny-G");
        tinyg_ref.feed_hold();
        match tinyg_ref.stop_gcode() {
            Err(msg) => warn!("Error when stopping gcode: {}", msg),
            Ok(()) => {
                info!("Stopped gcode sending.");
                tinyg_ref.flush_queue();
                tinyg_ref.cycle_start();
                match tinyg_ref.end_program() {
                    Err(msg) => warn!("Error when sending program end: {}", msg),
                    _ => {}
                }
            }
        }
    });


    let start_line_button : gtk4::Button = builder.object::<gtk4::Button>("start_line_button").unwrap();
    start_line_button.connect_clicked(clone!(@weak text_view, @weak start_mark => move |_button| {
        let (strong, _weak) = text_view
            .cursor_locations(None);

        let buffer = text_view
            .buffer();

        let line = text_view.iter_at_location(strong.x, strong.y).unwrap().line();

        buffer.remove_tag(&disabled_line_tag, &buffer.start_iter(), &buffer.end_iter());
        let iter = buffer.start_iter();
        buffer.apply_tag(&disabled_line_tag, &iter, &buffer.iter_at_line(line).unwrap());
        buffer.move_mark(&start_mark, &buffer.iter_at_line(line).unwrap());

         G_RENDER.lock().expect("").set_start_line(line as u32);
    }));

    builder.object::<gtk4::Button>("refallhome_button").unwrap().connect_clicked(|_button| {
        match TINY_G.lock().expect("Unable to lock Tiny-G").home_all() {
            Ok(_) => {

            },
            Err(msg) => {
                error!("Ref All Home: {}", msg);
            }
        }
    });

    builder.object::<gtk4::Button>("zerox_button").unwrap().connect_clicked(|_button| {
        match TINY_G.lock().expect("Unable to lock Tiny-G").zero_x() {
            Ok(_) => {

            },
            Err(msg) => {
                error!("Zero X: {}", msg);
            }
        }
    });

    builder.object::<gtk4::Button>("zeroy_button").unwrap().connect_clicked(|_button| {
        match TINY_G.lock().expect("Unable to lock Tiny-G").zero_y() {
            Ok(_) => {

            },
            Err(msg) => {
                error!("Zero Y: {}", msg);
            }
        }
    });

    builder.object::<gtk4::Button>("zeroz_button").unwrap().connect_clicked(|_button| {
        match TINY_G.lock().expect("Unable to lock Tiny-G").zero_z() {
            Ok(_) => {

            },
            Err(msg) => {
                error!("Zero Z: {}", msg);
            }
        }
    });

    builder.object::<gtk4::Button>("zeroa_button").unwrap().connect_clicked(|_button| {
        match TINY_G.lock().expect("Unable to lock Tiny-G").zero_a() {
            Ok(_) => {

            },
            Err(msg) => {
                error!("Zero A: {}", msg);
            }
        }
    });

    builder.object::<gtk4::Button>("cycle_start_button").unwrap().connect_clicked(|_button| {
        TINY_G2.lock().expect("Unable to lock Tiny-G").cycle_start();
    });

    builder.object::<gtk4::Button>("feed_hold_button").unwrap().connect_clicked(|_button| {
        TINY_G2.lock().expect("Unable to lock Tiny-G").feed_hold();
    });

    builder.object::<gtk4::Button>("reset_button").unwrap().connect_clicked(|_button| {
        TINY_G2.lock().expect("Unable to lock Tiny-G").reset();
    });

    let full_button: gtk4::ToggleButton = builder.object("move_full_radio").unwrap();
    let tenth_button: gtk4::ToggleButton = builder.object("move_tenth_radio").unwrap();
    let hundreds_button: gtk4::ToggleButton = builder.object("move_hundreds_radio").unwrap();
    let thousands_button: gtk4::ToggleButton = builder.object("move_thousands_radio").unwrap();
    let distance_input: gtk4::Entry = builder.object("distance_entry").unwrap();

    builder.object::<gtk4::Button>("x_minus_button").unwrap().connect_clicked(clone!(@weak full_button, @weak tenth_button, @weak hundreds_button, @weak thousands_button, @weak distance_input => move |_button| {
        let distance = get_selected_distance(full_button, tenth_button, hundreds_button, thousands_button, distance_input);
        match TINY_G.lock().expect("Unable to lock Tiny-G").move_xyza(Some(-distance), None, None, None) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("X Minus: {}", msg);
            }
        }
    }));

    builder.object::<gtk4::Button>("x_plus_button").unwrap().connect_clicked(clone!(@weak full_button, @weak tenth_button, @weak hundreds_button, @weak thousands_button, @weak distance_input => move |_button| {
        let distance = get_selected_distance(full_button, tenth_button, hundreds_button, thousands_button, distance_input);
        match TINY_G.lock().expect("Unable to lock Tiny-G").move_xyza(Some(distance), None, None, None) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("X Plus: {}", msg);
            }
        }
    }));

    builder.object::<gtk4::Button>("y_minus_button").unwrap().connect_clicked(clone!(@weak full_button, @weak tenth_button, @weak hundreds_button, @weak thousands_button, @weak distance_input => move |_button| {
        let distance = get_selected_distance(full_button, tenth_button, hundreds_button, thousands_button, distance_input);
        match TINY_G.lock().expect("Unable to lock Tiny-G").move_xyza(None, Some(-distance), None, None) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("Y Minus: {}", msg);
            }
        }
    }));

    builder.object::<gtk4::Button>("y_plus_button").unwrap().connect_clicked(clone!(@weak full_button, @weak tenth_button, @weak hundreds_button, @weak thousands_button, @weak distance_input => move |_button| {
        let distance = get_selected_distance(full_button, tenth_button, hundreds_button, thousands_button, distance_input);
        match TINY_G.lock().expect("Unable to lock Tiny-G").move_xyza(None, Some(distance), None, None) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("Y Plus: {}", msg);
            }
        }
    }));

    builder.object::<gtk4::Button>("x_minus_y_minus_button").unwrap().connect_clicked(clone!(@weak full_button, @weak tenth_button, @weak hundreds_button, @weak thousands_button, @weak distance_input => move |_button| {
        let distance = get_selected_distance(full_button, tenth_button, hundreds_button, thousands_button, distance_input);
        match TINY_G.lock().expect("Unable to lock Tiny-G").move_xyza(Some(-distance), Some(-distance), None, None) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("X Minus Y Minus: {}", msg);
            }
        }
    }));

    builder.object::<gtk4::Button>("x_minus_y_plus_button").unwrap().connect_clicked(clone!(@weak full_button, @weak tenth_button, @weak hundreds_button, @weak thousands_button, @weak distance_input => move |_button| {
        let distance = get_selected_distance(full_button, tenth_button, hundreds_button, thousands_button, distance_input);
        match TINY_G.lock().expect("Unable to lock Tiny-G").move_xyza(Some(-distance), Some(distance), None, None) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("X Minus Y Plus: {}", msg);
            }
        }
    }));

    builder.object::<gtk4::Button>("x_plus_y_minus_button").unwrap().connect_clicked(clone!(@weak full_button, @weak tenth_button, @weak hundreds_button, @weak thousands_button, @weak distance_input => move |_button| {
        let distance = get_selected_distance(full_button, tenth_button, hundreds_button, thousands_button, distance_input);
        match TINY_G.lock().expect("Unable to lock Tiny-G").move_xyza(Some(distance), Some(-distance), None, None) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("X Plus Y Minus: {}", msg);
            }
        }
    }));

    builder.object::<gtk4::Button>("x_plus_y_plus_button").unwrap().connect_clicked(clone!(@weak full_button, @weak tenth_button, @weak hundreds_button, @weak thousands_button, @weak distance_input => move |_button| {
        let distance = get_selected_distance(full_button, tenth_button, hundreds_button, thousands_button, distance_input);
        match TINY_G.lock().expect("Unable to lock Tiny-G").move_xyza(Some(distance), Some(distance), None, None) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("X Plus Y Plus: {}", msg);
            }
        }
    }));

    builder.object::<gtk4::Button>("z_minus_button").unwrap().connect_clicked(clone!(@weak full_button, @weak tenth_button, @weak hundreds_button, @weak thousands_button, @weak distance_input => move |_button| {
        let distance = get_selected_distance(full_button, tenth_button, hundreds_button, thousands_button, distance_input);
        match TINY_G.lock().expect("Unable to lock Tiny-G").move_xyza(None, None, Some(-distance), None) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("Z Minus: {}", msg);
            }
        }
    }));

    builder.object::<gtk4::Button>("z_plus_button").unwrap().connect_clicked(clone!(@weak full_button, @weak tenth_button, @weak hundreds_button, @weak thousands_button, @weak distance_input => move |_button| {
        let distance = get_selected_distance(full_button, tenth_button, hundreds_button, thousands_button, distance_input);
        match TINY_G.lock().expect("Unable to lock Tiny-G").move_xyza(None, None, Some(distance), None) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("Z Plus: {}", msg);
            }
        }
    }));

    let rpm_input: gtk4::Entry = builder.object("rpm_entry").unwrap();

    builder.object::<gtk4::Button>("spindle_cw_button").unwrap().connect_clicked(clone!(@weak rpm_input => move |_button| {
        let speed = get_selected_rpm(rpm_input);
        match TINY_G.lock().expect("Unable to lock Tiny-G").spindle_cw(speed) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("Spindle CW: {}", msg);
            }
        }
    }));

    builder.object::<gtk4::Button>("spindle_ccw_button").unwrap().connect_clicked(clone!(@weak rpm_input => move |_button| {
        let speed = get_selected_rpm(rpm_input);
        match TINY_G.lock().expect("Unable to lock Tiny-G").spindle_ccw(speed) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("Spindle CW: {}", msg);
            }
        }
    }));

    builder.object::<gtk4::Button>("spindle_stop_button").unwrap().connect_clicked(|_| {
        match TINY_G.lock().expect("Unable to lock Tiny-G").spindle_stop() {
            Ok(_) => {

            },
            Err(msg) => {
                error!("Spindle Stop: {}", msg);
            }
        }
    });

    let pos_x: gtk4::Label = builder.object("pos_x").unwrap();
    let pos_y: gtk4::Label = builder.object("pos_y").unwrap();
    let pos_z: gtk4::Label = builder.object("pos_z").unwrap();
    let pos_a: gtk4::Label = builder.object("pos_a").unwrap();

    main_window.show();

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
    G_RENDER.lock().expect("Unable to lock G_RENDER").set_position(old_status.posx, old_status.posy, old_status.posz);

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

    let g54 : gtk4::ToggleButton = builder.object("g54").unwrap();
    g54.connect_clicked(|_| {
        match TINY_G.lock().expect("Unable to lock Tiny-G").set_coordinate_sytem(1) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("G54: {}", msg);
            }
        }
    });
    let g55 : gtk4::ToggleButton = builder.object("g55").unwrap();
    g55.connect_clicked(|_| {
        match TINY_G.lock().expect("Unable to lock Tiny-G").set_coordinate_sytem(2) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("G55: {}", msg);
            }
        }
    });
    let g56 : gtk4::ToggleButton = builder.object("g56").unwrap();
    g56.connect_clicked(|_| {
        match TINY_G.lock().expect("Unable to lock Tiny-G").set_coordinate_sytem(3) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("G56: {}", msg);
            }
        }
    });
    let g57 : gtk4::ToggleButton = builder.object("g57").unwrap();
    g57.connect_clicked(|_| {
        match TINY_G.lock().expect("Unable to lock Tiny-G").set_coordinate_sytem(4) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("G57: {}", msg);
            }
        }
    });
    let g58 : gtk4::ToggleButton = builder.object("g58").unwrap();
    g58.connect_clicked(|_| {
        match TINY_G.lock().expect("Unable to lock Tiny-G").set_coordinate_sytem(5) {
            Ok(_) => {

            },
            Err(msg) => {
                error!("G58: {}", msg);
            }
        }
    });
    let g59 : gtk4::ToggleButton = builder.object("g59").unwrap();
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
                let buffer = text_view.buffer();
                let iter = buffer.iter_at_line(status.line as i32 + 5);
                if iter.is_some() {
                    let mark = buffer.create_mark(None, &iter.unwrap(), false);
                    text_view.scroll_mark_onscreen(&mark);
                    buffer.delete_mark(&mark);
                }

                buffer.remove_tag(&current_line_tag, &buffer.start_iter(), &buffer.end_iter());
                let iter = buffer.iter_at_line(status.line as i32).unwrap();
                let end_iter = buffer.iter_at_line(status.line as i32 + 1);
                if end_iter.is_some() {
                    buffer.apply_tag(&current_line_tag, &iter, &end_iter.unwrap());
                }
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
                stop_button.set_sensitive(true);
                start_line_button.set_sensitive(false);
                jog_box.set_sensitive(false);
                spindle_box.set_sensitive(false);
                position_box.set_sensitive(false);
            }
            Message::ProgrammStopped() => {
                file_choose_button.set_sensitive(true);
                start_button.set_sensitive(true);
                stop_button.set_sensitive(false);
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

    /*
    let _ = whb_thread.1.send(());
    whb_thread.0.join().unwrap();
    let _ = comm_thread.1.send(());
    comm_thread.0.join().unwrap();
     */
}
