//! Brewed Texture Designer, the core: everything about a texture graph that does not need a GPU or a window.
//!
//! * [`nodes`]: the registry of node types (sockets, parameters, metadata). Front ends and
//!   the engine are driven by these descriptions, so adding a node type does not touch the UI.
//! * [`graph`]: the document data (nodes, links, graph variables) and its invariants.
//! * [`package`] / [`instance`]: what a project file holds (graphs and the files they use),
//!   and graphs used as nodes in other graphs.
//! * [`expr`]: the expression language of the Math and Pixel Math nodes.
//! * [`eval`]: CPU evaluation of value nodes and of the effective parameters of every node.
//! * [`edit`] / [`document`]: all modifications as commands, with undo / redo.
//! * [`project`] / [`archive`]: package files (`.brewtex`, a ZIP archive: see
//!   `docs/package-format.md`), and paths relative to them.
//! * [`files`] / [`recovery`]: the files packages use or hold, and unsaved work kept for
//!   recovery after a crash.

pub mod archive;
pub mod curve;
pub mod demo;
pub mod document;
pub mod edit;
pub mod eval;
pub mod expr;
pub mod files;
pub mod gradient;
pub mod graph;
pub mod history;
pub mod instance;
pub mod nodes;
pub mod package;
pub mod project;
pub mod recovery;
pub mod templates;
pub mod types;

pub use document::Document;
pub use edit::{Edit, EditResult};
pub use curve::Curve;
pub use gradient::{Gradient, GradientStop, Interpolation};
pub use graph::{Clipboard, Comment, CommentId, Graph, GraphAttributes, Link, Node, NodeId, Precision, SocketInfo, SocketKind, SocketRef, Variable};
pub use instance::{GraphId, GraphRef, Instance, Interface};
pub use package::{Package, PackageEdit, Resource, ResourceKind};
pub use nodes::{Category, NodeDef, NodeKind, ParamDef, ParamKind, Role};
pub use types::{DataType, ParamValue, Tiling, Value, can_connect};

/// Name shown in window titles and dialogs.
pub const APP_NAME: &str = "Brewed Texture Designer";
