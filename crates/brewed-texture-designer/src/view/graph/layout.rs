//! Geometry of nodes on the canvas: sizes, socket positions, hit testing.

use iced::{Point, Rectangle, Size, Vector};
use brewed_texture_core::graph::{Comment, CommentId, Graph, Link, Node, NodeId, SocketInfo};
use brewed_texture_core::nodes::Role;

pub const NODE_W: f32 = 168.0;
pub const WIDE_NODE_W: f32 = 196.0;
pub const HEADER_H: f32 = 24.0;
pub const ROW_H: f32 = 20.0;
pub const THUMB: f32 = 136.0;
pub const LINE_H: f32 = 18.0;
pub const SOCKET_R: f32 = 5.5;

/// Canvas-local screen position = pan + graph position * zoom.
#[derive(Clone, Copy, Debug)]
pub struct Transform {
    pub pan: Vector,
    pub zoom: f32,
}

impl Transform {
    pub fn to_screen(self, p: [f32; 2]) -> Point {
        Point::new(self.pan.x + p[0] * self.zoom, self.pan.y + p[1] * self.zoom)
    }

    pub fn to_graph(self, p: Point) -> [f32; 2] {
        [(p.x - self.pan.x) / self.zoom, (p.y - self.pan.y) / self.zoom]
    }
}

pub fn node_width(node: &Node) -> f32 {
    if node.is_expression_node() { WIDE_NODE_W } else { NODE_W }
}

/// Lines of text in the body of value nodes (expression, value).
fn body_lines(node: &Node) -> f32 {
    match node.def().role {
        Role::Value if node.is_expression_node() => 2.0,
        _ => 1.0,
    }
}

/// Size of a node in graph units.
pub fn node_size(graph: &Graph, node: &Node) -> [f32; 2] {
    let rows = graph.input_sockets(node).len().max(graph.output_sockets(node).len()).max(1) as f32;
    let mut body = 0.0;
    if node.def().has_image() {
        if node.is_expression_node() {
            body += LINE_H;
        }
        body += THUMB + 8.0;
    } else {
        body += body_lines(node) * LINE_H;
    }
    [node_width(node), HEADER_H + 4.0 + rows * ROW_H + body + 6.0]
}

pub struct NodeLayout {
    pub id: NodeId,
    pub rect: Rectangle,
    pub header: Rectangle,
    pub inputs: Vec<(SocketInfo, Point)>,
    pub outputs: Vec<(SocketInfo, Point)>,
    pub thumb: Option<Rectangle>,
    /// Top of the body (below the socket rows).
    pub body_top: f32,
}

pub fn layout(graph: &Graph, node: &Node, t: &Transform, offset: Vector) -> NodeLayout {
    let [w, h] = node_size(graph, node);
    let z = t.zoom;
    let min = t.to_screen([node.pos[0] + offset.x, node.pos[1] + offset.y]);
    let rect = Rectangle::new(min, Size::new(w * z, h * z));
    let header = Rectangle::new(min, Size::new(rect.width, HEADER_H * z));
    let row_y = |i: usize| min.y + (HEADER_H + 2.0 + (i as f32 + 0.5) * ROW_H) * z;
    let inputs: Vec<(SocketInfo, Point)> = graph
        .input_sockets(node)
        .into_iter()
        .enumerate()
        .map(|(i, s)| (s, Point::new(rect.x, row_y(i))))
        .collect();
    let outputs: Vec<(SocketInfo, Point)> = graph
        .output_sockets(node)
        .into_iter()
        .enumerate()
        .map(|(i, s)| (s, Point::new(rect.x + rect.width, row_y(i))))
        .collect();
    let rows = inputs.len().max(outputs.len()).max(1) as f32;
    let mut body_top = min.y + (HEADER_H + 4.0 + rows * ROW_H) * z;
    let mut thumb = None;
    if node.def().has_image() {
        if node.is_expression_node() {
            body_top += LINE_H * z;
        }
        let s = THUMB * z;
        thumb = Some(Rectangle::new(Point::new(rect.center_x() - s * 0.5, body_top + 2.0 * z), Size::new(s, s)));
    }
    NodeLayout { id: node.id, rect, header, inputs, outputs, thumb, body_top }
}

/// Endpoints of a link (output socket, input socket), if both are visible.
pub fn link_endpoints(layouts: &[NodeLayout], link: &Link) -> Option<(Point, Point)> {
    let from = layouts.iter().find(|l| l.id == link.from)?;
    let to = layouts.iter().find(|l| l.id == link.to)?;
    let a = from.outputs.iter().find(|(s, _)| s.key == link.from_socket)?.1;
    let b = to.inputs.iter().find(|(s, _)| s.key == link.to_socket)?.1;
    Some((a, b))
}

/// Control points of the cubic curve of a link.
pub fn bezier(a: Point, b: Point, zoom: f32) -> [Point; 4] {
    let dx = ((b.x - a.x).abs() * 0.5).max(40.0 * zoom);
    [a, Point::new(a.x + dx, a.y), Point::new(b.x - dx, b.y), b]
}

fn cubic(p: &[Point; 4], t: f32) -> Point {
    let u = 1.0 - t;
    let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    Point::new(
        a * p[0].x + b * p[1].x + c * p[2].x + d * p[3].x,
        a * p[0].y + b * p[1].y + c * p[2].y + d * p[3].y,
    )
}

/// Distance from `p` to the curve (sampled).
pub fn bezier_distance(points: &[Point; 4], p: Point) -> f32 {
    let mut best = f32::INFINITY;
    let mut prev = points[0];
    for i in 1..=24 {
        let q = cubic(points, i as f32 / 24.0);
        let seg = q - prev;
        let len2 = seg.x * seg.x + seg.y * seg.y;
        let t = if len2 > 0.0 { (((p.x - prev.x) * seg.x + (p.y - prev.y) * seg.y) / len2).clamp(0.0, 1.0) } else { 0.0 };
        let c = Point::new(prev.x + seg.x * t, prev.y + seg.y * t);
        best = best.min(c.distance(p));
        prev = q;
    }
    best
}

// ------------------------------------------------------------------------- comments

/// Height of the title bar of a comment, in graph units: the part that can be grabbed to
/// move it. The rest of a comment lets the mouse through to the nodes and the canvas.
pub const COMMENT_HEADER_H: f32 = 30.0;
/// The description of a comment under its title: its size, how far apart its lines are (in
/// sizes), and how far it keeps from the frame's sides and title.
pub const DESCRIPTION_SIZE: f32 = 12.5;
pub const DESCRIPTION_LINE: f32 = 1.35;
pub const DESCRIPTION_PAD: f32 = 8.0;
/// Side of the corner that resizes a comment, in graph units.
pub const COMMENT_GRIP: f32 = 18.0;

pub struct CommentLayout {
    pub id: CommentId,
    pub rect: Rectangle,
    pub header: Rectangle,
    /// Bottom-right corner, dragged to resize.
    pub grip: Rectangle,
}

/// Where a comment is on the screen. `offset` moves it while it is dragged, `resize`
/// replaces its size while a corner is dragged.
pub fn comment_layout(comment: &Comment, t: &Transform, offset: Vector, resize: Option<[f32; 2]>) -> CommentLayout {
    let size = resize.unwrap_or(comment.size);
    let z = t.zoom;
    let min = t.to_screen([comment.pos[0] + offset.x, comment.pos[1] + offset.y]);
    let rect = Rectangle::new(min, Size::new(size[0] * z, size[1] * z));
    let header = Rectangle::new(min, Size::new(rect.width, (COMMENT_HEADER_H * z).min(rect.height)));
    let side = (COMMENT_GRIP * z).min(rect.width).min(rect.height);
    let grip = Rectangle::new(
        Point::new(rect.x + rect.width - side, rect.y + rect.height - side),
        Size::new(side, side),
    );
    CommentLayout { id: comment.id, rect, header, grip }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brewed_texture_core::Precision;
    use brewed_texture_core::templates::{Example, Template};

    /// How far down the description of `comment` reaches, at most, below its top: its lines
    /// wrapped as wide as the frame is, counted with the widest letters the font has on
    /// average (so it says more lines than there are, never fewer).
    fn said_to(comment: &Comment) -> f32 {
        if comment.description.is_empty() {
            return COMMENT_HEADER_H;
        }
        let per_line = (((comment.size[0] - 2.0 * DESCRIPTION_PAD) / (DESCRIPTION_SIZE * 0.58)) as usize).max(1);
        let lines: usize = comment
            .description
            .split('\n')
            .map(|paragraph| {
                // Word by word, as the text wraps.
                let mut lines = 1;
                let mut line = 0;
                for word in paragraph.split(' ') {
                    let n = word.chars().count();
                    if line > 0 && line + 1 + n > per_line {
                        lines += 1;
                        line = n;
                    } else {
                        line += if line > 0 { 1 + n } else { n };
                    }
                }
                lines
            })
            .sum();
        COMMENT_HEADER_H + DESCRIPTION_PAD + lines as f32 * DESCRIPTION_SIZE * DESCRIPTION_LINE + DESCRIPTION_PAD
    }

    /// Where something is in a graph, and how large: its top left corner and its size.
    type Place = ([f32; 2], [f32; 2]);

    fn overlap(a: Place, b: Place) -> bool {
        let (ap, asz) = a;
        let (bp, bsz) = b;
        ap[0] < bp[0] + bsz[0] && bp[0] < ap[0] + asz[0] && ap[1] < bp[1] + bsz[1] && bp[1] < ap[1] + asz[1]
    }

    /// The graphs the program makes itself, the examples of Learn and the templates of a new
    /// graph, are laid out as someone careful would: no node over another, every node wholly
    /// in a frame or out of it, below what the frame says; and every graph of an example says
    /// what it does, in a frame's description.
    #[test]
    fn examples_and_templates_are_laid_out_apart() {
        let mut graphs: Vec<(String, Graph, bool)> = Vec::new();
        for example in Example::ALL {
            for entry in example.package().0.graphs {
                graphs.push((format!("{} / {}", example.name(), entry.name), Graph::clone(&entry.graph), true));
            }
        }
        for template in Template::ALL {
            graphs.push((template.name().into(), template.graph([512, 512], Precision::Half), false));
        }
        let mut wrong = Vec::new();
        for (name, g, example) in &graphs {
            let boxes: Vec<(&Node, Place)> = g.nodes.iter().map(|n| (n, (n.pos, node_size(g, n)))).collect();
            for (i, (a, ab)) in boxes.iter().enumerate() {
                for (b, bb) in &boxes[i + 1..] {
                    if overlap(*ab, *bb) {
                        wrong.push(format!("{name}: {} over {}", a.def().name, b.def().name));
                    }
                }
            }
            for c in &g.comments {
                for (n, (pos, size)) in &boxes {
                    if c.holds(*pos, *size) {
                        if pos[1] < c.pos[1] + said_to(c) {
                            wrong.push(format!("{name}: {} over what '{}' says (below {})", n.def().name, c.text, c.pos[1] + said_to(c)));
                        }
                    } else if overlap((c.pos, c.size), (*pos, *size)) {
                        wrong.push(format!("{name}: {} half in '{}'", n.def().name, c.text));
                    }
                }
            }
            if *example && !g.comments.iter().any(|c| !c.description.is_empty()) {
                wrong.push(format!("{name} says nothing of what it does"));
            }
        }
        assert!(wrong.is_empty(), "{wrong:#?}");
    }
}
