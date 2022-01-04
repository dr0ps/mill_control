use glium::implement_vertex;

#[derive(Copy, Clone)]
pub struct Vertex {
    pub position: [f32; 3],
    pub base_color: [f32; 3],
}

implement_vertex!(Vertex, position, base_color);

