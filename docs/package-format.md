# The package format (`.brewtex`), version 1

A package is what Brewed Texture Designer saves: any number of graphs, the files they use
(pictures, vector graphics, fonts) and how the editor last showed them, in one file. The file can
be copied, mailed or put in version control on its own: files the package *holds* travel inside
it, and files it *links to* are named relative to it wherever they can be.

The format is meant to be read and written by other programs too: an asset pipeline that lists
the graphs of a package and the textures they export, a tool that pulls the pictures out, another
editor. It is a ZIP archive of JSON documents and files, so any language reads it with the
tools it already has, and its rules for growing are those of glTF: what a reader does not know,
it passes over; what it must not pass over, it is told about.

The reference implementation is `crates/brewed-texture-core` (`project.rs` for the package,
`archive.rs` for the ZIP subset, `graph.rs` for graphs).

## At a glance

```
walls.brewtex                          a ZIP archive
├── mimetype                           application/vnd.brewed-texture.package+zip (first, stored)
├── package.json                       the manifest: the graphs, the files and their names
├── editor.json                        which graph was open, what each 2D and 3D view showed
├── thumbnail.png                      a picture of the package, for lists of packages
├── graphs/1.json                      a graph each, by its id
├── graphs/4.json
├── files/9f86d081…0f00a08.png         a file the package holds, by its SHA-256
└── x-studio/review.txt                what another program keeps with the package
```

`mimetype` is a file like the others, of one line: the type of the file, where a program that
tells files by their first bytes finds it (see *The archive*). It registers nothing anywhere:
EPUB books and OpenDocument files begin the same way.

Reading a package with Python, say:

```python
import hashlib, json, zipfile

with zipfile.ZipFile("walls.brewtex") as z:
    manifest = json.loads(z.read("package.json"))
    assert manifest["format"] == "brewed-texture-package" and manifest["version"] <= 1
    for graph in manifest["graphs"]:
        g = json.loads(z.read(f"graphs/{graph['id']}.json"))
        outputs = [n for n in g["nodes"] if n["type"] == "output"]
        print(graph["name"], len(g["nodes"]), "nodes,", len(outputs), "outputs")
    for f in manifest.get("files", []):
        if f["file"].startswith("embedded:"):  # held by the package
            held = f["file"].removeprefix("embedded:")
            data = z.read("files/" + held)
            assert hashlib.sha256(data).hexdigest() == held.split(".")[0]
            print(f["name"], len(data), "bytes")
```

## The archive

A package file is a ZIP archive as PKWARE's APPNOTE (6.3) describes it, within these limits:

- Entries are **stored** (method 0) or **deflated** (method 8); nothing is encrypted; the archive
  is one part (not split over disks).
- **ZIP64** records are allowed, and needed wherever a size, an offset or the number of entries
  goes past what the classic fields hold. A reader takes them where a classic field is full
  (`0xFFFF`, `0xFFFFFFFF`), as the specification says.
- Sizes and CRC-32s may follow an entry in a data descriptor; a reader takes them from the
  central directory.
- Entry names are UTF-8 (general purpose bit 11 says so where a name is more than ASCII) and
  separate their parts by `/`. No part is empty, `.` or `..`; no name holds `\`, `:` or NUL;
  no two entries have the same name.

The first entry is **`mimetype`**, stored, without an extra field: its bytes,
`application/vnd.brewed-texture.package+zip` (no line break), are at offset 38 of the file,
where a program that tells files by their first bytes finds them (as it finds those of EPUB and
OpenDocument files). A reader should not require it (a package zipped again by another tool may
have it elsewhere, deflated or not at all), but when it is there, it holds exactly that.

Brewed Texture Designer writes every entry's sizes and CRC-32 before its bytes, gives every entry
the time 1980-01-01 00:00 and the attributes `rw-r--r--`, and writes the entries in a fixed order
(`mimetype`, `package.json`, `editor.json`, the graphs, the files, other entries): the same
package is the same bytes whenever it is saved, and differences in version control are
differences of the package. Other writers need not do so.

## `package.json`

What the package is. A JSON object:

| Field | Type | |
|---|---|---|
| `format` | string | Always `"brewed-texture-package"`. |
| `version` | integer | The version of the format: `1`. |
| `generator` | string | What wrote the file, e.g. `"Brewed Texture Designer 0.2.0"`. Optional. |
| `graphs` | array | The graphs, in the order the package lists them: at least one (below). |
| `files` | array | The files the package uses (below). Optional. |
| `next_graph` | integer | The id the next graph made gets: above every id a graph of the package has had, so a graph taken out never gives its id to another. Optional. |
| `extensions_used` | array of strings | The extensions of the format the package uses. Optional. |
| `extensions_required` | array of strings | Those of them a reader must know to read the package. Optional. |
| `extras` | object | What other programs keep with the package. Optional. |

A graph:

| Field | Type | |
|---|---|---|
| `id` | integer | Its id in the package, from 1: what nodes that use it find it by. It stays the same when the graph is renamed. |
| `name` | string | Its name, its own in the package. |

The graph is the entry `graphs/<id>.json`.

A file:

| Field | Type | |
|---|---|---|
| `file` | string | The file, as the package's nodes say it: `embedded:<sha256>.<ext>` for a file the package holds (see *Files the package holds*), else where it is on disk (see *Paths*). |
| `name` | string | What the package calls it, its own in the package: the name the file had at first, renamed as people like. Optional: the file's name. |
| `extras` | object | What other programs keep with the file. Optional. |

What a file is (a picture, read by a Bitmap; vector graphics, by an SVG node; a font, by a
Text node) is its extension's to say: `png`, `jpg`, `jpeg`, `tga`, `bmp`, `tif`, `tiff`, `exr`,
`hdr`; `svg`, `svgz`; `ttf`, `otf`, `ttc`.

Nothing is said twice. Where a graph or a file is in the archive, what kind of file a file is,
which packages the graphs use: all of it follows from what is said, and is not said again (a
second saying could only disagree with the first).

```json
{
  "format": "brewed-texture-package",
  "version": 1,
  "generator": "Brewed Texture Designer 0.2.0",
  "graphs": [
    { "id": 1, "name": "Brick Wall" },
    { "id": 4, "name": "Mortar" }
  ],
  "files": [
    { "file": "embedded:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08.png", "name": "Bricks, old" },
    { "file": "fonts/Inter.ttf", "name": "Inter.ttf" }
  ],
  "next_graph": 5
}
```

A reader takes what it can do without as it can: a file of the list it cannot read (no `file`,
or a file of no kind above) is left out, and the reader says so; `generator`, `next_graph`,
`name`, `extras` or `extensions_used` that say what cannot be taken (a number where text goes,
`-1` for a count) are as they are when not said. What it cannot do without (the format, the
version, the graphs, `extensions_required`) it reads as it is, or refuses the package, saying
why: rather than read it wrongly and save it without what it did not understand.

## Files the package holds

Each file the package holds is one entry, `files/<sha256>.<ext>`: its bytes as they are, named
by the SHA-256 of them in 64 small hexadecimal digits (as `sha256sum` writes it) and the
extension of the file it came from, in small letters (what reads it may go by it: a TGA picture
does not say what it is). Pictures are stored (they are compressed already); vector graphics
and fonts are deflated. What the package calls the file is its `name` in the manifest: renaming
the file renames nothing in the archive.

Nodes name such a file `embedded:<sha256>.<ext>`. The name says the bytes: the same name is the
same file in any package, a node copied into another package brings its file along, and a file
taken in twice is held once. A reader checks the hash: bytes that are not what their name says
are not taken (the package opens, the file is missing, and the reader says so).

## Paths

A file the package links to (does not hold) is named where it is on disk, relative to the
folder the package's file is in: `textures/bricks.png` is in the folder `textures` beside
`walls.brewtex`, `../fonts/Inter.ttf` in the folder `fonts` beside the folder of
`walls.brewtex`. Relative, a package moved with its files keeps finding them. Parts are
separated by `/` on every system; a path is absolute where the two share no root (another drive
on Windows). A reader takes a relative path from the folder the package is in. A path is never
an `embedded:` name, and an `embedded:` name is never a path.

A package whose graph uses a graph of another package as a node names that package so too (a
node's `uses.package`, see *Nodes*). Such a package depends on that other file, as one that
links to a file does: it is whole only with it. A package that holds its files and uses only its
own graphs needs nothing beside it.

## Graphs

`graphs/<id>.json` holds one graph, a JSON object:

| Field | Type | |
|---|---|---|
| `resolution` | `[width, height]` | The size of its images, 16 to 8192 each. |
| `precision` | string | What its images are kept in: `unorm8`, `float16` or `float32`. Optional: `float16`. |
| `nodes` | array | Its nodes (below). |
| `links` | array | Its links (below). |
| `input_values` | array | Its input values (below). Optional. |
| `comments` | array | Frames drawn around groups of nodes (below). Optional. |
| `next_id` | integer | The id the next node or comment gets: above every id of the graph. |
| `attributes` | object | What the graph says of itself, for the Library: `description` (what it makes), `category` (where the Library shows it, `Noises/Grunge`; none: under the package's name), `hidden` (`true`: not in the Library). Each optional, and the object too. |
| `extras` | object | What other programs keep with the graph. Optional. |

Ids of nodes and comments are integers from 1, below 2<sup>48</sup>, each its own in the graph
(nodes and comments share them). A reader gives ids of 2<sup>48</sup> and above new ones, as it
holds a resolution to 16…8192.

### Nodes

| Field | Type | |
|---|---|---|
| `id` | integer | Its id in the graph. |
| `type` | string | What it is: `blur`, `tile_generator`, `output`… (see *Node types*). |
| `position` | `[x, y]` | Where it is in the graph view, in its units (±10<sup>7</sup> at most). |
| `params` | object | Its parameters by key, in their order, each a value (below). |
| `exposed` | array of strings | The keys of the parameters exposed as sockets, which links may drive. Optional. |
| `input_value` | string | The input value of the graph an Input node reads (`float_input`, `image_input`…). Optional. |
| `locals` | object | The values of the variables of an expression (Math, Pixel Math) that nothing is linked to, by name. Optional. |
| `bypass` | bool | The node passes its first input through as it is. Optional: `false`. |
| `uses` | object | The graph a node of type `graph_instance` makes (below). |
| `extras` | object | What other programs keep with the node. Optional. |

A value says its type: `{"float": 0.5}`, `{"int": 4}`, `{"bool": true}`, `{"vec2": [0.5, 1.0]}`,
`{"color": [r, g, b, a]}` (straight, not premultiplied: the values an image holds), `{"text": "…"}` (text, an
expression, or a file: see *Paths*), `{"gradient": {…}}`, `{"curve": {…}}`, or
`{"choice": "<id>"}`: one of the options of a choice, by its id (`{"choice": "directx"}`), never
by its place in a list.

A gradient is `{"stops": [{"pos": 0.0, "color": [r, g, b, a]}, …], "interpolation": "linear"}`
(`linear`, `smooth` or `constant`); a curve, `{"points": [[x, y], …]}`, from 0 to 1 both.

A node that uses a graph (`graph_instance`) says which in `uses`, and what the graph was as a
node when it was saved, so its sockets and values stay while the graph cannot be found:

| Field | Type | |
|---|---|---|
| `package` | string | The package file the graph is in (see *Paths*). Not there: this package. |
| `graph` | integer | The graph's id in that package. |
| `name` | string | Its name when the node last saw it: what the graph is found by if its package has no graph of that id any more. |
| `interface` | object | Its `outputs` (`[{"name": …}]`, one for each name of its Output nodes), its `params`: its input values (below, with a `label` each), which are the node's parameters, and its `description` (what the graph says it makes; optional). |

### Links

`{"from": 3, "from_socket": "o:value", "to": 7, "to_socket": "p:x_amount"}`: from an output of
one node to an input of another. At most one link arrives at an input. Sockets are named by what
they are:

| Socket | |
|---|---|
| `o:<key>` | An output of the node: of its type (`o:output`), or, of a graph used as a node, an Output of that graph by its name. |
| `i:<key>` | An image input of the node type (`i:input`, `i:mask`…). |
| `p:<key>` | A parameter exposed as a socket; of a graph used as a node, an input value of that graph. |
| `v:<name>` | A variable of the expression of a Math or Pixel Math node. |

### Input values

What a graph takes from outside: shared by its Input nodes of the name, and the parameters of a
node that uses the graph.

| Field | Type | |
|---|---|---|
| `name` | string | Its name, its own in the graph (what Input nodes read). |
| `type` | string | `float`, `int`, `bool`, `vec2`, `color` or `image`. |
| `value`, `default` | value | Its value and its value by default (for an image, its color while it has no picture). |
| `min`, `max` | value | The range of its slider. |
| `description` | string | Optional. |
| `image` | object | For an image: `{"kind": "gray" or "color", "picture": <file>, "default": <file>}`: what it takes, its picture and its picture by default (see *Paths*; empty: none). |

### Comments

`{"id": 27, "text": "The shape of the bricks", "description": "A Tile Generator lays…", "position": [x, y], "size": [w, h], "color": [r, g, b, a]}`

`text` is its title; `description` (optional) is said under it, wrapped to its width.

## `editor.json`

How the editor showed the package (optional: a package without it opens at its first graph):

```json
{ "open": 1, "views": [{ "graph": 1, "preview": 22, "model": "sphere", "relief": 0.04, "repeat": 1.0,
                         "light": { "panorama": { "output": 5 } }, "exposure": 0.0 }] }
```

`open` is the graph being edited; each view says the node a graph's 2D view showed (`preview`)
and the scene of its 3D view: the model (`model`: `rounded_cube`, `sphere`, `matball`, `cube`,
`rounded_cylinder`, `cylinder`, `torus`, `plane` or `cloth`), how high the view raises it by its
height (`relief`) and how many times the texture goes round it (`repeat`), what lights it
(`light`: `"studio"`, `{ "image": { "file": "…" } }` — an environment image, relative to the
package where it can be, as other paths — or `{ "panorama": { "output": 5 } }`, the image of an
Output of the graph whose usage is Panorama) and how brightly, in stops (`exposure`). Each is
optional: what a view does not say is left as the editor has it. Where the graph view was, and
how near, is not kept, nor where the camera of the 3D view was: a package opens with each graph
shown whole, and moving a view is no change of the package (a `pan` or `zoom` of a file written
before is not read).

## `thumbnail.png`

A picture of the package, a PNG (optional): what lists of packages show of it, the Home screen's.
Brewed Texture Designer writes the thumbnail of the first Output of the graph being edited (else
of the node its 2D view shows) when it saves the package, and the one the file had when it saves
a package it opened without editing it. A reader shows it, and never needs it.

## Growing the format

- A reader **passes over** what it does not know: a field, an entry, a parameter of a node that
  its version of the node does not have.
- `extras` objects (of the package, a file, a graph, a node) hold what other programs keep: a
  program that does not know them writes them back as they were. So are the entries of the
  archive outside `mimetype`, `package.json`, `editor.json`, `recovery.json`, `thumbnail.png`, `graphs/` and
  `files/`: Brewed Texture Designer writes them back as they were (not the folders some tools
  list, which the names of their entries make).
- An **extension** is named in `extensions_used`, and in `extensions_required` if a reader that
  does not know it would read the package wrongly: such a reader refuses the package instead of
  reading it wrongly and saving it without what it did not know. Names of extensions are
  prefixed by who made them (`studio_terrain`). This version knows none.
- **`version`** grows only when a reader of the version before would read a file wrongly. A
  reader refuses a package of a version above its own.
- A node of a **type** a reader does not know, or a choice of an **option** it does not know
  (made by a newer version of the program), makes it refuse the package, saying which: what it
  cannot read it would lose.

### Node types

The node types, their sockets, their parameters with the types of their values, and the ids of
the options of their choices are listed in `crates/brewed-texture-core/tests/fixtures/nodes.txt`.
A test sees that everything listed there stays: a name in a file keeps meaning what it meant
(things are added, never renamed or taken out).

## Snapshots

A snapshot is what the program keeps of a package with unsaved changes, to offer it back after a
crash (`<settings>/Recovery/<session>/<n>.brewtex`). It is a package file with one more entry,
`recovery.json` (`{"path": <file it is saved as, if it is>, "title": …, "time": <seconds since
1970>}`), whose paths are as they are in memory (absolute), and whose files are kept beside it
(`<sha256>.held`), once for all the snapshots of a session, instead of in it.

The earlier versions of a package, kept as it is saved (`<settings>/History/<hash of where the
package is>/<milliseconds>-<n>-<saved|found|replaced>.brewtex`), are snapshots too, whose files
are kept once for all the versions of every package (`<settings>/History/files/<sha256>.held`).
A version of a file that could not be read as a package (`found`, kept before it was written
over) is that file, byte for byte.

## Limits of the reader

Brewed Texture Designer reads packages of at most 100 000 entries, with a directory of at most
64 MB; `package.json`, `editor.json` and each graph of at most 256 MB; each file a package holds
of at most 2 GB; and other programs' entries of at most 64 MB each, 256 MB in all. An entry is
read as far as it may be, never to more than it says it holds, and is checked by its CRC-32: an
archive that says it is small and unpacks to gigabytes is refused, not read.
