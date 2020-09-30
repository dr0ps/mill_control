use iced::{slider, Column, Element, ProgressBar, Sandbox, Settings, Slider};
use std::process::exit;
use crate::tinyg::Tinyg;
use std::io::{stdout, Write};

mod tinyg;

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

    Progress::run(Settings::default())
}

#[derive(Default)]
struct Progress {
    value: f32,
    progress_bar_slider: slider::State,
}

#[derive(Debug, Clone, Copy)]
enum Message {
    SliderChanged(f32),
}

impl Sandbox for Progress {
    type Message = Message;

    fn new() -> Self {
        Self::default()
    }

    fn title(&self) -> String {
        String::from("Hallo!")
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::SliderChanged(x) => self.value = x,
        }
    }

    fn view(&mut self) -> Element<Message> {
        Column::new()
            .padding(20)
            .push(ProgressBar::new(0.0..=100.0, self.value))
            .push(
                Slider::new(
                    &mut self.progress_bar_slider,
                    0.0..=100.0,
                    self.value,
                    Message::SliderChanged,
                ),
            )
            .into()
    }
}
