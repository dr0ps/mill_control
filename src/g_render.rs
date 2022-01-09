use std::f32::consts::PI;
use gcode::{Parser, Mnemonic, Nop, buffers::DefaultBuffers, GCode};
use glium::backend::{Context, Facade};
use glium::index::IndicesSource;
use glium::index::PrimitiveType::{LineStrip, TrianglesList};
use glium::{Blend, BlendingFunction, DrawParameters, Program, Surface, uniform, VertexBuffer};
use gtk4::GLArea;
use crate::gl_facade::GLFacade;
use crate::gl_area_backend::GLAreaBackend;
use crate::stylus::create_stylus;
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
    angle_y: f32,
    zoom: f32,
    last_active_line: u32,
    stylus: Vec<Vertex>,
    pos_x: f32,
    pos_y: f32,
    pos_z: f32,
    auto_reset_rotation: bool,
    start_line: u32,
}

fn create_vertex_g1(code : &GCode, loc_x: &mut f32, loc_y: &mut f32, loc_z: &mut f32, absolute : bool, line: u32) -> Vertex {
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
    Vertex{position: [*loc_x, *loc_y, *loc_z], base_color: match code.major_number() { 1 => [0.0, 1.0, 0.0], _ => [1.0, 1.0, 0.0]}, line }
}

fn add(gcode_pos: Option<f32>, current: & f32, absolute: bool) -> f32 {
    return if gcode_pos.is_some() {
        if absolute {
            gcode_pos.unwrap()
        } else {
            gcode_pos.unwrap() + *current
        }
    } else {
        *current
    }
}

#[test]
fn test_add_none_absolute() {
    let current = 10.0;
    assert_eq!(10.0, add(None, &current, true));
}

#[test]
fn test_add_none_relative() {
    let current = 10.0;
    assert_eq!(10.0, add(None, &current, false));
}

#[test]
fn test_add_some_absolute() {
    let current = 10.0;
    assert_eq!(1.0, add(Some(1.0), &current, true));
}

#[test]
fn test_add_some_relative() {
    let current = 10.0;
    assert_eq!(11.0, add(Some(1.0), &current, false));
}

fn create_vertex_g3(code : &GCode, loc_x: &mut f32, loc_y: &mut f32, loc_z: &mut f32, absolute : bool, line: u32) -> Vec<Vertex> {

    let mut result = Vec::new();

    let x= add(code.value_for('x'), loc_x, absolute);

    let y= add(code.value_for('y'), loc_y, absolute);

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
            result.push(Vertex{position: [x, y, *loc_z], base_color: [0.0, 0.0, 1.0], line});
        }
    } else { // counter-clockwise, increasing radians
        if angle2 < angle1 {
            angle2 += 2.0*PI;
        }
        let angle_range = angle2 - angle1;
        let step = angle_range / radius / 10.0;
        let mut current_angle = angle1;

        while current_angle < angle2
        {
            let x = center_x + radius * current_angle.cos();
            let y = center_y + radius * current_angle.sin();
            current_angle += step;
            result.push(Vertex{position: [x, y, *loc_z], base_color: [0.0, 0.0, 1.0], line});
        }

    }
    result.push(Vertex{position: [*loc_x, *loc_y, *loc_z], base_color: [0.0, 0.0, 1.0], line});
    result
}

fn dot_product(m1 : [[f32; 4]; 4], m2 : [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut result =[
        [0.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 0.0]
    ];

    for i in 0..4 {
        for j in 0..4 {
            for k in 0..4 {
                result[i][j] += m1[i][k] * m2[k][j];
            }
        }
    }
    result
}

#[test]
fn test_dot_product_identity() {
    let a = [
        [1.0, 2.0, 3.0, 4.0],
        [5.0, 6.0, 7.0, 8.0],
        [9.0, 0.0, 1.0, 2.0],
        [3.0, 4.0, 5.0, 6.0]
    ];

    let identiy = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0]
    ];

    assert_eq!(a, dot_product(a, identiy));
    assert_eq!(a, dot_product(identiy, a));
}


impl GRender {
    pub fn new() -> Self {
        Self { line:Vec::new(),
            min_x:0.0,
            max_x:0.0,
            min_y:0.0,
            max_y:0.0,
            min_z:0.0,
            max_z:0.0,
            width: 100,
            height: 100,
            angle_x: 0.0,
            angle_y: 0.0,
            zoom: 1.0,
            last_active_line: 0,
            stylus: Vec::new(),
            pos_x: 0.0,
            pos_y: 0.0,
            pos_z: 0.0,
            auto_reset_rotation: true,
            start_line: 0
        }
    }

    pub fn initialize(&mut self, gl_area : &GLArea) -> (GLFacade, Program){
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
        #version 300 es
        in vec3 position;
        in vec3 base_color;
        out vec3 b_color;
        out float depth;
        uniform mat4 matrix;
        uniform mat4 rotation;
        uniform mat4 translation;
        uniform mat4 perspective;
        void main() {
            b_color = base_color;
            gl_Position = perspective * translation * rotation * matrix * vec4(position, 1.0);
            depth = gl_Position.z;
        }
    "#;

        let fragment_shader_src = r#"
        #version 300 es
        in mediump float depth;
        in mediump vec3 b_color;
        out mediump vec4 color;
        void main() {
            mediump float brightness = 2.5-depth;
            mediump vec3 regular_color = b_color;
            mediump vec3 dark_color = b_color * 0.1;
            color = vec4(mix(dark_color, regular_color, brightness), 1.0);
        }
    "#;

        create_stylus(&mut self.stylus);

        let program = Program::from_source(&gl_context, vertex_shader_src, fragment_shader_src, None).unwrap();
        (gl_context, program)
    }

    pub fn update(&mut self, contents : &str) -> Result<(), String>{
        self.line.clear();
        self.last_active_line = 0;
        self.min_x = 0.0;
        self.max_x = 0.0;
        self.min_y = 0.0;
        self.max_y = 0.0;
        self.min_z = 0.0;
        self.max_z = 0.0;
        let mut absolute = true;
        let mut loc_x : f32 = 0.0;
        let mut loc_y : f32 = 0.0;
        let mut loc_z : f32 = 0.0;

        let lines: Parser<Nop, DefaultBuffers> = Parser::new(&contents, Nop);
        let mut line_number = 0;
        for line in lines {
            for code in line.gcodes() {
                match code.mnemonic() {
                    Mnemonic::General => match code.major_number() {
                        0 | 1 => {
                            self.line.push(create_vertex_g1(code, &mut loc_x, &mut loc_y, &mut loc_z, absolute, line_number ))
                        }
                        2 | 3 => {
                            self.line.append(&mut create_vertex_g3(code, &mut loc_x, &mut loc_y, &mut loc_z, absolute, line_number))
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
            line_number += 1;
        }
        Ok(())

    }

    pub fn draw(&mut self, facade : &GLFacade, program : &Program)  {
        let context = facade.get_context();
        let mut frame =
            glium::Frame::new(context.clone(), context.get_framebuffer_dimensions());

        let start = self.line.partition_point(|probe| probe.line < self.start_line);
        let vertex_buffer = VertexBuffer::persistent(facade, self.line.split_at(start).1).unwrap();

        frame.clear_color(0.0, 0.0, 0.0, 1.0);

        let initial_translation = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [-(self.pos_x.max(self.max_x)+self.pos_x.min(self.min_x))/2.0,
                -(self.pos_y.max(self.max_y)+self.pos_y.min(self.min_y))/2.0,
                -(self.pos_z.max(self.max_z)+self.pos_z.min(self.min_z))/2.0, 1.0f32]
        ];

        let initial_scale_factor = 1.0 / (self.pos_x.max(self.max_x)-self.pos_x.min(self.min_x))
            .max(self.pos_y.max(self.max_y)-self.pos_y.min(self.min_y))
            .max(self.pos_z.max(self.max_z)-self.pos_z.min(self.min_z));

        let initial_scale = [
            [initial_scale_factor, 0.0, 0.0, 0.0],
            [0.0, -initial_scale_factor, 0.0, 0.0],
            [0.0, 0.0, initial_scale_factor, 0.0],
            [0.0, 0.0, 0.0, 1.0f32]
        ];

        let initial_rotation = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, (-0.5*PI).cos(), (-0.5*PI).sin(), 0.0],
            [0.0, -(-0.5*PI).sin(), (-0.5*PI).cos(), 0.0],
            [0.0, 0.0, 0.0, 1.0f32]
        ];

        if self.auto_reset_rotation {
            self.angle_y = self.angle_y * 0.95;
            self.angle_x = self.angle_x * 0.95;
        }

        let rotation = dot_product(
        [
            [self.angle_x.cos(), 0.0, -self.angle_x.sin(), 0.0],
            [0.0, 1.0, 0.1, 0.0],
            [self.angle_x.sin(), 0.0, self.angle_x.cos(), 0.0],
            [0.0, 0.0, 0.0, 1.0f32]
        ],
        [
             [1.0, 0.0, 0.0, 0.0],
             [0.0, self.angle_y.cos(), self.angle_y.sin(), 0.0],
             [0.0, -self.angle_y.sin(), self.angle_y.cos(), 0.0],
             [0.0, 0.0, 0.0, 1.0f32]
         ]
        );

        let translation = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 2.0, 1.0f32]
        ];

        let zoom = [
            [1.3_f32.powf(self.zoom), 0.0, 0.0, 0.0],
            [0.0, 1.3_f32.powf(self.zoom), 0.0, 0.0],
            [0.0, 0.0, 1.3_f32.powf(self.zoom), 0.0],
            [0.0, 0.0, 0.0, 1.0f32]
        ];

        let perspective = {
            let aspect_ratio = self.height as f32 / self.width as f32;

            let fov: f32 = PI / 3.0;
            let zfar = 1024.0;
            let znear = 0.1;

            let f = 1.0 / (fov / 2.0).tan();

            [
                [f *   aspect_ratio   ,    0.0,              0.0              ,   0.0],
                [         0.0         ,     f ,              0.0              ,   0.0],
                [         0.0         ,    0.0,  (zfar+znear)/(zfar-znear)    ,   1.0],
                [         0.0         ,    0.0, -(2.0*zfar*znear)/(zfar-znear),   0.0],
            ]
        };

        let matrix = dot_product(dot_product(dot_product(initial_translation, initial_scale), initial_rotation), zoom);

        let draw_parameters = DrawParameters {
            blend: Blend {
                color: BlendingFunction::Max,
                alpha: BlendingFunction::Max,
                .. Default::default()
            },
            .. Default::default()
        };

        frame.draw((&vertex_buffer, &vertex_buffer), IndicesSource::NoIndices {primitives : LineStrip}, program, &uniform! { matrix: matrix, rotation: rotation, translation: translation, perspective: perspective },
                   &draw_parameters).unwrap();

        let stylus_translation = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [-(self.pos_x.max(self.max_x)+self.pos_x.min(self.min_x))/2.0 + self.pos_x,
                -(self.pos_y.max(self.max_y)+self.pos_y.min(self.min_y))/2.0 + self.pos_y,
                -(self.pos_z.max(self.max_z)+self.pos_z.min(self.min_z))/2.0 + self.pos_z + 8.0, 1.0f32]
        ];

        let matrix = dot_product(dot_product(dot_product(stylus_translation, initial_scale), initial_rotation), zoom);

        let stylus_buffer = VertexBuffer::persistent(facade, self.stylus.as_slice()).unwrap();
        frame.draw((&stylus_buffer, &stylus_buffer), IndicesSource::NoIndices {primitives : TrianglesList}, program, &uniform! { matrix: matrix, rotation: rotation, translation: translation, perspective: perspective },
                   &draw_parameters).unwrap();

        frame.finish().unwrap();
        vertex_buffer.invalidate();
    }

    pub fn resize(&mut self, width: i32, height: i32) {
        self.width = width;
        self.height = height;
    }

    pub fn set_angle(&mut self, pos_x: f32, pos_y: f32) {
        self.angle_x = PI * (self.width as f32 / 2.0 - pos_x) / self.width as f32;
        self.angle_y = PI * (self.height as f32 / 2.0 - pos_y) / self.height as f32;
    }

    pub fn set_zoom(&mut self, zoom: f32) {
        self.zoom += zoom;
    }

    pub fn update_line(&mut self, line: u32) {
        for n in self.last_active_line as usize  .. self.line.len() {
            let vertex = self.line.get_mut(n).unwrap();
            if vertex.line < line {
                vertex.base_color = [0.0, 0.0, 0.0];
            }
            else if vertex.line == line {
                vertex.base_color = [0.0, 0.0, 0.0];
                self.last_active_line = n as u32;
            }
            else if vertex.line < line + 10 {
                vertex.base_color = [1.0, 1.0, 1.0];
            }
            else {
                break;
            }
        }
    }

    pub fn set_position(&mut self, pos_x: f32, pos_y: f32, pos_z: f32) {
        self.pos_x = pos_x;
        self.pos_y = pos_y;
        self.pos_z = pos_z;
    }

    pub fn enable_auto_reset_rotation(&mut self)
    {
        self.auto_reset_rotation = true;
    }

    pub fn disable_auto_reset_rotation(&mut self)
    {
        self.auto_reset_rotation = false;
    }

    pub fn set_start_line(&mut self, line : u32) {
        self.start_line = line;
    }
}
