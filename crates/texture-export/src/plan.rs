//! The maps a program has, and the files an export writes of them.

/// What a map is, by the id a program and its templates know it by: of a material's maps, the
/// ids of the program's usages ("base_color", "roughness", "normal"…: here those of the
/// Output's usage, see `brewed_texture_core::nodes::Usage::id`); of a map of no usage,
/// `custom:` and its name.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct MapId(pub String);

impl MapId {
    /// A map of no usage, by its name.
    pub fn custom(name: &str) -> MapId {
        MapId(format!("custom:{name}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// What it is called in a sentence: "no ambient occlusion".
    pub fn word(&self) -> String {
        self.0.strip_prefix("custom:").unwrap_or(&self.0).replace('_', " ")
    }

    /// Whether the map is gray: one channel of it is written as a map of three. A map of a color,
    /// of a direction (a normal map, a tangent), a panorama, and one of no usage (which may be
    /// either) are not; every other usage is a gray map of a value.
    pub fn gray(&self) -> bool {
        let id = self.as_str();
        !(id.starts_with("custom:")
            || id.ends_with("color")
            || self.is_normal()
            || matches!(id, "emissive" | "diffuse" | "specular" | "reflection" | "refraction" | "panorama" | "tangent" | "coat_tangent" | "ambient"))
    }

    /// Whether it is a normal map: its green turned over to be the other way's.
    pub fn is_normal(&self) -> bool {
        matches!(self.as_str(), "normal" | "coat_normal" | "detail_normal")
    }
}

impl From<&str> for MapId {
    fn from(id: &str) -> MapId {
        MapId(id.into())
    }
}

/// A map a program has: its reference of its own to it (what [`Source`] and [`Planned`] give
/// back), its name, and what it is, where it is one of what templates read.
#[derive(Clone, Debug, PartialEq)]
pub struct Map<R> {
    pub reference: R,
    pub name: String,
    pub usage: Option<MapId>,
}

/// What one channel of a file packed of several is.
#[derive(Clone, Debug, PartialEq)]
pub enum Source<R> {
    /// A channel of a map (0 red … 3 alpha; a grayscale map is the same in the first three),
    /// turned over (1 − it) where `invert`.
    Map { map: R, channel: u8, invert: bool },
    /// A value, where the program has no map of what it would be.
    Value(f32),
}

/// A file an export writes.
#[derive(Clone, Debug, PartialEq)]
pub enum Planned<R> {
    /// A map, as the program has it (its name, channels and format: the maps as they are).
    AsItIs(R),
    /// Channels of maps, or values: gray of one; red, green and blue of three; and alpha of
    /// four. What a template writes, in the channels and format it says.
    Pack(Vec<Source<R>>),
}

/// A file an export writes: its name (without extension), what it is made of, and what that
/// is, said.
#[derive(Clone, Debug, PartialEq)]
pub struct PlannedFile<R> {
    pub name: String,
    pub what: Planned<R>,
    pub said: String,
    /// What it is written as, where its template says (else as the export says).
    pub format: Option<crate::Format>,
}

/// The files of `maps` as the program has them: each with the name, channels and format it has.
pub fn as_they_are<R: Copy>(maps: &[Map<R>]) -> Vec<PlannedFile<R>> {
    maps.iter().map(|m| PlannedFile { name: m.name.clone(), what: Planned::AsItIs(m.reference), said: m.name.clone(), format: None }).collect()
}
