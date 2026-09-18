//! Ferrotex — a GPU accelerated, node based procedural texture editor.
//!
//! * [`graph`]: the document model (nodes, typed sockets, links, graph inputs)
//! * [`expr`]: the expression language of the Math nodes
//! * [`engine`]: incremental evaluation of the graph on the GPU
//! * [`gpu`]: wgpu context and WGSL compute kernels
//! * [`simd`]: multithreaded SIMD kernels for CPU-side pixel work
//! * [`ui`] / [`app`]: the egui front end

pub mod app;
pub mod demo;
pub mod engine;
pub mod expr;
pub mod gpu;
pub mod graph;
pub mod history;
pub mod simd;
pub mod tools;
pub mod ui;
