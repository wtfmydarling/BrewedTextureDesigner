//! Tessera core: everything about a texture graph that does not need a GPU or a window.
//!
//! * [`nodes`]: the registry of node types (sockets, parameters, metadata). Front ends and
//!   the engine are driven by these descriptions, so adding a node type does not touch the UI.
//! * [`graph`]: the document data (nodes, links, graph variables) and its invariants.
//! * [`expr`]: the expression language of the Math and Pixel Math nodes.
//! * [`eval`]: CPU evaluation of value nodes and of the effective parameters of every node.
//! * [`edit`] / [`document`]: all modifications as commands, with undo / redo.
//! * [`project`]: the file format (with migration of older files).

pub mod demo;
pub mod document;
pub mod edit;
pub mod eval;
pub mod expr;
pub mod gradient;
pub mod graph;
pub mod history;
pub mod nodes;
pub mod project;
pub mod types;

pub use document::Document;
pub use edit::{Edit, EditResult};
pub use gradient::{Gradient, GradientStop, Interpolation};
pub use graph::{Clipboard, Comment, CommentId, Graph, Link, Node, NodeId, SocketInfo, SocketKind, SocketRef, Variable};
pub use nodes::{Category, NodeDef, NodeKind, ParamDef, ParamKind, Role};
pub use types::{DataType, ParamValue, Tiling, Value, can_connect};

/// Name shown in window titles and dialogs.
pub const APP_NAME: &str = "Tessera";
