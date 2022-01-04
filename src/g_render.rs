use std::f32::consts::PI;
use gcode::{Parser, Mnemonic, Nop, buffers::DefaultBuffers, GCode};
use glium::backend::{Context, Facade};
use glium::index::IndicesSource;
use glium::index::PrimitiveType::LineStrip;
use glium::{Program, Surface, uniform, VertexBuffer};
use gtk::GLArea;
use log::{info, warn};
use crate::gl_facade::GLFacade;
use crate::gl_area_backend::GLAreaBackend;
use crate::vertex::Vertex;

pub struct GRender {
    line : Vec<Vertex>,
    min_x: f32,
    max_x: f32,
    min_y: f32,
    max_y: f32,
    min_z: f32,
    max_z: f32,
    width: i32,
    height: i32,
    angle_x: f32,
}

fn create_vertex_g1(code : &GCode, loc_x: &mut f32, loc_y: &mut f32, loc_z: &mut f32, absolute : bool) -> Vertex {
    if let Some(z) = code.value_for('z') {
        if absolute {
            *loc_z = z.into();
        } else {
            *loc_z += z as f32;
        }
    }
    if let Some(x) = code.value_for('x') {
        if absolute {
            *loc_x = x.into();
        } else {
            *loc_x += x as f32;
        }
    }
    if let Some(y) = code.value_for('y') {
        if absolute {
            *loc_y = y.into();
        } else {
            *loc_y += y as f32;
        }
    }
    Vertex{position: [*loc_x, *loc_y, *loc_z], base_color: [0.0, 1.0, 0.0]}
}

fn create_vertex_g3(code : &GCode, loc_x: &mut f32, loc_y: &mut f32, loc_z: &mut f32, absolute : bool) -> Vec<Vertex> {

    let mut result = Vec::new();

    let x;
    let x_pos = code.value_for('x');
    if x_pos.is_some() {
        if absolute {
            x = x_pos.unwrap()
        } else {
            x = x_pos.unwrap() + *loc_x
        };
    }
    else {
        x = *loc_x;
    }

    let y;
    let y_pos = code.value_for('y');
    if y_pos.is_some() {
        if absolute {
            y = y_pos.unwrap()
        } else {
            y= y_pos.unwrap() + *loc_y
        };
    }
    else {
        y = *loc_y;
    }

    // TODO: handle i or j not being entered
    let center_x;
    let center_y;
    let radius;
    if let (Some(i), Some(j)) = (code.value_for('i'), code.value_for('j'))
    {
        center_x = *loc_x + i as f32;
        center_y = *loc_y + j as f32;
        radius = ((center_x - *loc_x).powi(2) + (center_y - *loc_y).powi(2)).sqrt();
    } else if let Some(r) = code.value_for('r') {
        radius = r as f32;

        let q = ((x - *loc_x).powi(2) + (y - *loc_y).powi(2)).sqrt();

        let y3 = (*loc_y + y) / 2.;
        let x3 = (*loc_x + x) / 2.;

        let basex = (radius.powi(2) - (q / 2.).powi(2)).sqrt() * ((*loc_y - y) / q);
        let basey = (radius.powi(2) - (q / 2.).powi(2)).sqrt() * ((x - *loc_x) / q);

        // TODO: center may be at -basex -basey, need to figure out how to pick
        center_x = x3 + basex;
        center_y = y3 + basey;
    }
    else {
        return result;
    }
    let mut angle1 = (*loc_y - center_y).atan2(*loc_x - center_x);
    let mut angle2 = (y - center_y).atan2(x - center_x);

    *loc_x = x;
    *loc_y = y;
    // TODO: figure out how wide the z drawing should be (above and below current z)
    if code.major_number() == 2 {  //clockwise, decreasing radians
        if angle2 > angle1 {
            angle1 += 2.0*PI; // 4.712388907
        }
        let angle_range = angle1 - angle2; // 1.583453607
        let step = angle_range / radius / 10.0; // 0.052781787
        let mut current_angle = angle1;

        while current_angle > angle2
        {
            let x = center_x + radius * current_angle.cos();
            let y = center_y + radius * current_angle.sin();
            current_angle -= step;
            result.push(Vertex{position: [x, y, *loc_z], base_color: [0.0, 0.0, 1.0]});
        }
    } else { // counter-clockwise, increasing radians
        if angle2 < angle1 {
            angle2 += 2.0*PI;
        }
        info!("Angles: {}, {}", angle1, angle2);
        let mut angle_range = angle2 - angle1;
        let step = angle_range / radius / 10.0;
        let mut current_angle = angle1;
        info!("Start, step, range: {}, {}, {}", current_angle, step, angle_range);

        while current_angle < angle2
        {
            let x = center_x + radius * current_angle.cos();
            let y = center_y + radius * current_angle.sin();
            current_angle += step;
            result.push(Vertex{position: [x, y, *loc_z], base_color: [0.0, 0.0, 1.0]});
        }

    }
    result.push(Vertex{position: [*loc_x, *loc_y, *loc_z], base_color: [0.0, 0.0, 1.0]});
    result
}

impl GRender {
    pub fn new() -> Self {
        Self { line:Vec::new(), min_x:0.0, max_x:0.0, min_y:0.0, max_y:0.0, min_z:0.0, max_z:0.0, width: 100, height: 100, angle_x: 0.0}
    }

    pub fn initialize(gl_area : &GLArea) -> (GLFacade, Program){
        let context = unsafe {
            Context::new(
                GLAreaBackend::new(gl_area.clone()),
                true,
                glium::debug::DebugCallbackBehavior::DebugMessageOnError,
            )
                .unwrap()
        };

        let gl_context = GLFacade::new(context);

        let vertex_shader_src = r#"
        #version 150
        in vec3 position;
        in vec3 base_color;
        out vec3 b_color;
        out float depth;
        uniform mat4 perspective;
        uniform mat4 matrix;
        uniform mat4 rotation;
        void main() {
            b_color = base_color;
            gl_Position = rotation * perspective * matrix  * vec4(position, 1.0);
            depth = gl_Position.z;
        }
    "#;

        let fragment_shader_src = r#"
        #version 150
        in float depth;
        in vec3 b_color;
        out vec4 color;
        uniform vec3 u_light;
        void main() {
            float brightness = 1-depth;
            vec3 regular_color = b_color;
            vec3 dark_color = b_color * 0.5;
            color = vec4(mix(dark_color, regular_color, brightness), 1.0);
        }
    "#;

        let program = Program::from_source(&gl_context, vertex_shader_src, fragment_shader_src, None).unwrap();
        (gl_context, program)
    }

    pub fn update(&mut self, contents : &str) -> Result<(), String>{
        self.line.clear();
        let mut absolute = true;
        let mut loc_x : f32 = 0.0;
        let mut loc_y : f32 = 0.0;
        let mut loc_z : f32 = 0.0;

        let lines: Parser<Nop, DefaultBuffers> = Parser::new(&contents, Nop);
        for line in lines {
            for code in line.gcodes() {
                match code.mnemonic() {
                    Mnemonic::General => match code.major_number() {
                        0 | 1 => {
                            self.line.push(create_vertex_g1(code, &mut loc_x, &mut loc_y, &mut loc_z, absolute))
                        }
                        2 | 3 => {
                            self.line.append(&mut create_vertex_g3(code, &mut loc_x, &mut loc_y, &mut loc_z, absolute))
                        }
                        90 => absolute = true,
                        91 => absolute = false,
                        _ => {}
                    },
                    _ => {}
                }
                if loc_x < self.min_x {
                    self.min_x = loc_x;
                }
                if loc_y < self.min_y {
                    self.min_y = loc_y;
                }
                if loc_z < self.min_z {
                    self.min_z = loc_z;
                }
                if loc_x > self.max_x {
                    self.max_x = loc_x;
                }
                if loc_y > self.max_y {
                    self.max_y = loc_y;
                }
                if loc_z > self.max_z {
                    self.max_z = loc_z;
                }
            }
        }
        Ok(())

    }

    pub fn draw(&mut self, facade : &GLFacade, program : &Program)  {
        let context = facade.get_context();
        let mut frame =
            glium::Frame::new(context.clone(), context.get_framebuffer_dimensions());

        let vertex_buffer = VertexBuffer::dynamic(facade, self.line.as_slice()).unwrap();

        frame.clear_color(0.0, 0.0, 0.0, 1.0);

        let matrix = [
            [0.01, 0.0, 0.0, 0.0],
            [0.0, 0.01, 0.0, 0.0],
            [0.0, 0.0, 0.01, 0.0],
            [0.0, 0.0, 0.0, 1.0f32]
        ];

        let angle :f32 = self.angle_x;
        self.angle_x += 0.01;

        let rotation = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, angle.cos(), angle.sin(), 0.0],
            [0.0, -angle.sin(), angle.cos(), 0.0],
            [0.0, 0.0, 0.0, 1.0f32]
        ];

        /*let perspective = {
            let aspect_ratio = self.height as f32 / self.width as f32;

            let fov: f32 = 3.141592 / 3.0;
            let zfar = 1024.0;
            let znear = 0.1;

            let f = 1.0 / (fov / 2.0).tan();

            [
                [f *   aspect_ratio   ,    0.0,              0.0              ,   0.0],
                [         0.0         ,     f ,              0.0              ,   0.0],
                [         0.0         ,    0.0,  (zfar+znear)/(zfar-znear)    ,   1.0],
                [         0.0         ,    0.0, -(2.0*zfar*znear)/(zfar-znear),   0.0],
            ]
        };*/
        let perspective = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0f32]
        ];


        let light = [-1.0, 0.4, 0.9f32];

        frame.draw((&vertex_buffer, &vertex_buffer), IndicesSource::NoIndices {primitives : LineStrip}, program, &uniform! { matrix: matrix, perspective: perspective, rotation: rotation, u_light: light },
                   &Default::default()).unwrap();
        frame.finish().unwrap();
    }

    pub fn resize(&mut self, width: i32, height: i32) {
        self.width = width;
        self.height = height;
    }
}
