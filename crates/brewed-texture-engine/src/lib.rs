//! Brewed Texture Designer, the engine: evaluates graphs on the GPU (wgpu compute shaders) and does the CPU
//! side pixel work with multithreaded SIMD.
//!
//! * [`gpu`]: device wrapper, WGSL kernels, image pool, readbacks
//! * [`kernels`]: the GPU implementation of every image node type
//! * [`engine`]: incremental evaluation, thumbnails, display copies, readbacks
//! * [`service`]: the engine on a background thread with a request / event protocol
//! * [`simd`]: `wide::f32x8` + `rayon` kernels (quantization, half floats, histograms)
//! * [`export`]: PNG / OpenEXR writing

pub mod engine;
pub mod export;
pub mod gpu;
pub mod kernels;
pub mod service;
pub mod simd;
pub mod tools;

#[cfg(test)]
mod tests;

pub use engine::{DisplayImage, Engine, EvalReport, ImageReport, Thumbnail};
pub use gpu::Gpu;
pub use service::{EngineService, Event, ExportJob, Request};

/// The wgpu version the engine is built against (front ends must render with the same).
pub use wgpu;
