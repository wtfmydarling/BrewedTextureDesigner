//! A widget of a pixel that draws nothing: iced makes its pipeline with the device it draws
//! the window with, and the program takes the device then for the engine (see `crate::gpu`).
//! The Home screen draws one, so that the engine starts and the GPU is known before any graph
//! is shown (the 2D and 3D views, which do the same, are not there).

use brewed_texture_engine::wgpu;
use iced::widget::shader::{self, Viewport};
use iced::{Element, Length, Rectangle, mouse};

use crate::app::Message;

struct Probe;

#[derive(Debug)]
pub struct Primitive;

pub struct Pipeline;

impl shader::Program<Message> for Probe {
    type State = ();
    type Primitive = Primitive;

    fn draw(&self, _state: &(), _cursor: mouse::Cursor, _bounds: Rectangle) -> Primitive {
        Primitive
    }
}

impl shader::Pipeline for Pipeline {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, _format: wgpu::TextureFormat) -> Self {
        crate::gpu::publish(device, queue);
        Pipeline
    }
}

impl shader::Primitive for Primitive {
    type Pipeline = Pipeline;

    fn prepare(&self, _pipeline: &mut Pipeline, _device: &wgpu::Device, _queue: &wgpu::Queue, _bounds: &Rectangle, _viewport: &Viewport) {}
}

/// The probe: a pixel, drawn as nothing.
pub fn probe<'a>() -> Element<'a, Message> {
    iced::widget::shader(Probe).width(Length::Fixed(1.0)).height(Length::Fixed(1.0)).into()
}
