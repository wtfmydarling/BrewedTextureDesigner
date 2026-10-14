//! What an export of textures writes, made to be the same in every program of a set: a program
//! says which maps it has (what each is, its name, and a reference of its own to it), and is
//! told the files a template writes of them — each a map as it is, or channels of maps (or
//! values) packed into one file — and what to name them. It knows nothing of the program, of
//! graphs or of the GPU: the program reads its maps and writes the files (see
//! `brewed_texture_engine::export` for this program's).
//!
//! A [`Template`] is data: the program's presets (those of game engines and formats, each
//! program's own, in its own words) and the user's own templates alike (see the `template`
//! module). The parts of a file's name (`$(graph)`…) are the program's too, given as it fills
//! them in (see the `naming` module). Where a program has no map of what a template reads, one
//! that stands for it is taken (a diffuse color for the base color, glossiness turned over for
//! the roughness, a displacement for the height), or a value where none does.

mod names;
pub mod naming;
mod plan;
mod template;

pub use names::{file_stem, sanitize_file_name, unique_paths};
pub use plan::{Map, MapId, Planned, PlannedFile, Source, as_they_are};
pub use template::{
    Edit, File, FileFormat, FileType, Format, Input, Layout, Missing, Normals, Part, Size, Template, new_template_path, plan_template, read_templates,
    template_of, write_template,
};
