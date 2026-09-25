//! Brewed Texture Designer, the engine: evaluates graphs on the GPU (WGSL kernels drawn with
//! wgpu) and does the CPU side pixel work with multithreaded SIMD.
//!
//! * [`gpu`]: device wrapper, WGSL kernels, image pool, readbacks
//! * [`kernels`]: the GPU implementation of every image node type
//! * [`engine`]: incremental evaluation, thumbnails, the images the views show, readbacks
//! * [`service`]: the engine on a background thread with a request / event protocol
//! * [`simd`]: `wide::f32x8` + `rayon` kernels (quantization, half floats, histograms)
//! * [`export`]: PNG / OpenEXR writing

pub mod engine;
pub mod export;
pub mod gpu;
pub mod kernels;
pub mod render;
pub mod service;
pub mod simd;
#[cfg(any(test, feature = "dev-tools"))]
pub mod tools;
pub mod vector;

#[cfg(test)]
mod tests;

pub use engine::{ChoicePictures, DisplayImage, Engine, EvalReport, ImageReport, Keep, Memory, Thumbnail};
pub use gpu::Gpu;
pub use service::{EngineService, Event, ExportJob, Request};

/// The wgpu version the engine is built against (front ends must render with the same).
pub use wgpu;
