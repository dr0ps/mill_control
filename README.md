# mill_control - a simple tool to control a TinyG cnc controller

This creates a single binary that is able to directly control a TinyG board.
* You can manually jog or send a gcode file. Loading a Gcode file will run it immediately.
* Feed hold works, cycle start continues the program.
* There is support for a WHB04B remote.

![screenshot](mill_control.png)

## Getting Started

mill_control is written in Rust.

### Prerequisites

You need a Rust/Cargo installation. See https://rustup.rs/. After checking out this repository you can simply run

```
cargo run
```

and if that works you can install the binary.


### Installing

You can build a binary for deployment by running

```
cargo build --release
```



## Authors

See the list of [contributors](https://github.com/dr0ps/mill_control/contributors) who participated in this project.

## License

This project is licensed under the GNU GENERAL PUBLIC LICENSE, Version 2 - see the [LICENSE](LICENSE) file for details

