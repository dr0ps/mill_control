use std::rc::Rc;
use glium::backend::{Context, Facade};

pub struct GLFacade {
    context: Rc<Context>,
}

impl Facade for GLFacade {
    fn get_context(&self) -> &Rc<Context> {
        &self.context
    }
}

impl GLFacade {
    pub fn new(context : Rc<Context>) -> GLFacade {
        GLFacade {context}
    }
}