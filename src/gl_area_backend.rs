use std::ffi::c_void;
use glium::SwapBuffersError;
use gtk::prelude::*;

pub struct GLAreaBackend {
    gl_area: gtk::GLArea,
}

unsafe impl glium::backend::Backend for GLAreaBackend {
    fn swap_buffers(&self) -> Result<(), SwapBuffersError> {
        // GTK swaps the buffers after each "render" signal itself
        Ok(())
    }
    unsafe fn get_proc_address(&self, symbol: &str) -> *const c_void {
        gl_loader::get_proc_address(symbol) as *const _
    }
    fn get_framebuffer_dimensions(&self) -> (u32, u32) {
        let allocation = self.gl_area.allocation();
        (allocation.width as u32, allocation.height as u32)
    }
    fn is_current(&self) -> bool {
        // GTK makes it current itself on each "render" signal
        true
    }
    unsafe fn make_current(&self) {
        self.gl_area.make_current();
    }
}

impl GLAreaBackend {
    pub fn new(gl_area: gtk::GLArea) -> Self {
        Self { gl_area }
    }
}
