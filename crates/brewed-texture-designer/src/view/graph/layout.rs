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
