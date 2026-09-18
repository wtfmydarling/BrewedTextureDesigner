//! UI self-test ("autopilot"): drives the real application with synthetic mouse and
//! keyboard events and checks the resulting graph after every step.
//!
//! Run with `ferrotex --ui-test`. Real pointer events are filtered out while it runs.

use std::collections::VecDeque;

use eframe::egui::{self, Event, Key, Modifiers, PointerButton, Pos2, pos2, vec2};

use super::FerrotexApp;
use crate::graph::{NodeId, NodeKind};

type Check = Box<dyn Fn(&FerrotexApp) -> Result<(), String>>;

pub struct AutoPilot {
    step: usize,
    frames: VecDeque<Vec<Event>>,
    wait_until: f64,
    check: Option<(String, Check)>,
    pub results: Vec<(String, Result<(), String>)>,
    pub finished: bool,
    pointer: Pos2,
}

fn press_ev(p: Pos2, button: PointerButton, pressed: bool) -> Event {
    Event::PointerButton { pos: p, button, pressed, modifiers: Modifiers::NONE }
}

fn key_ev(key: Key, pressed: bool, modifiers: Modifiers) -> Event {
    Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }
}

impl AutoPilot {
    pub fn new() -> AutoPilot {
        AutoPilot {
            step: 0,
            frames: VecDeque::new(),
            wait_until: 0.0,
            check: None,
            results: Vec::new(),
            finished: false,
            pointer: pos2(10.0, 10.0),
        }
    }

    /// Events to inject into the next frame.
    pub fn take_events(&mut self) -> Vec<Event> {
        self.frames.pop_front().unwrap_or_default()
    }

    // ------------------------------------------------------------------ event builders

    fn idle(&mut self, frames: usize) {
        for _ in 0..frames {
            self.frames.push_back(Vec::new());
        }
    }

    fn move_to(&mut self, p: Pos2) {
        self.pointer = p;
        self.frames.push_back(vec![Event::PointerMoved(p)]);
    }

    fn click_with(&mut self, p: Pos2, button: PointerButton) {
        self.move_to(p);
        self.frames.push_back(vec![press_ev(p, button, true)]);
        self.frames.push_back(vec![press_ev(p, button, false)]);
        self.idle(2);
    }

    fn click(&mut self, p: Pos2) {
        self.click_with(p, PointerButton::Primary);
    }

    fn double_click(&mut self, p: Pos2) {
        self.move_to(p);
        for _ in 0..2 {
            self.frames.push_back(vec![press_ev(p, PointerButton::Primary, true)]);
            self.frames.push_back(vec![press_ev(p, PointerButton::Primary, false)]);
        }
        self.idle(2);
    }

    fn drag(&mut self, from: Pos2, to: Pos2) {
        self.move_to(from);
        self.frames.push_back(vec![press_ev(from, PointerButton::Primary, true)]);
        let steps = 10;
        for i in 1..=steps {
            let p = from + (to - from) * (i as f32 / steps as f32);
            self.pointer = p;
            self.frames.push_back(vec![Event::PointerMoved(p)]);
        }
        self.frames.push_back(vec![press_ev(to, PointerButton::Primary, false)]);
        self.idle(3);
    }

    fn key(&mut self, key: Key, modifiers: Modifiers) {
        let mut down = Vec::new();
        if modifiers != Modifiers::NONE {
            down.push(Event::ModifiersChanged(modifiers));
        }
        down.push(key_ev(key, true, modifiers));
        self.frames.push_back(down);
        let mut up = vec![key_ev(key, false, modifiers)];
        if modifiers != Modifiers::NONE {
            up.push(Event::ModifiersChanged(Modifiers::NONE));
        }
        self.frames.push_back(up);
        self.idle(2);
    }

    fn text(&mut self, s: &str) {
        self.frames.push_back(vec![Event::Text(s.to_string())]);
        self.idle(2);
    }

    // ------------------------------------------------------------------ driver

    /// Called at the end of every frame.
    pub fn update(&mut self, app: &FerrotexApp, now: f64) {
        if self.finished || !self.frames.is_empty() || now < self.wait_until {
            return;
        }
        if let Some((name, check)) = self.check.take() {
            let result = check(app);
            match &result {
                Ok(()) => println!("  PASS  {name}"),
                Err(e) => println!("  FAIL  {name}: {e}"),
            }
            self.results.push((name, result));
        }
        match self.plan(app) {
            Some((name, settle, check)) => {
                self.check = Some((name.to_string(), check));
                self.wait_until = now + settle;
                self.step += 1;
            }
            None => {
                self.finished = true;
                let failed = self.results.iter().filter(|r| r.1.is_err()).count();
                println!("UI self-test: {} passed, {} failed", self.results.len() - failed, failed);
            }
        }
    }

    /// Queue the events of the next step; returns its name, how long to let the app
    /// settle afterwards, and the check to run.
    fn plan(&mut self, app: &FerrotexApp) -> Option<(&'static str, f64, Check)> {
        const LAST_STEP: usize = 22;
        let step = self.step;
        if step > LAST_STEP {
            return None;
        }
        match self.plan_step(app, step) {
            Some(p) => Some(p),
            None => {
                let msg = format!("preconditions of step {step} not met (widget or node not found)");
                Some(("step setup", 0.0, Box::new(move |_: &FerrotexApp| Err(msg.clone()))))
            }
        }
    }

    fn plan_step(&mut self, app: &FerrotexApp, step: usize) -> Option<(&'static str, f64, Check)> {
        let canvas = app.view.canvas_rect();
        let at = |fx: f32, fy: f32| canvas.min + vec2(canvas.width() * fx, canvas.height() * fy);
        let lib = |label: &str| app.library.item_rects.iter().find(|(l, _)| *l == label).map(|(_, r)| r.center());
        let node_of = |kind: NodeKind| app.graph.nodes.iter().rev().find(|n| n.kind == kind).map(|n| n.id);
        let socket = |id: NodeId, key: &str, output: bool| {
            app.view
                .debug_nodes
                .iter()
                .find(|d| d.id == id)
                .and_then(|d| d.sockets.iter().find(|(k, o, _)| k == key && *o == output).map(|s| s.2))
        };
        let header = |id: NodeId| app.view.debug_nodes.iter().find(|d| d.id == id).map(|d| d.header);
        let body = |id: NodeId| app.view.debug_nodes.iter().find(|d| d.id == id).map(|d| d.rect);
        let fail = |msg: String| -> Check { Box::new(move |_| Err(msg.clone())) };

        match step {
            0 => {
                self.idle(5);
                Some(("demo graph evaluates without errors", 1.5, Box::new(|app: &FerrotexApp| {
                    if app.graph.nodes.len() != 21 {
                        return Err(format!("expected 21 demo nodes, got {}", app.graph.nodes.len()));
                    }
                    for n in &app.graph.nodes {
                        if let Some(e) = app.engine.state(n.id).and_then(|s| s.error.clone()) {
                            return Err(format!("{:?}: {e}", n.kind));
                        }
                    }
                    if app.engine.preview_info(app.preview.node.ok_or("no preview node")?).is_none() {
                        return Err("2D view has no texture".into());
                    }
                    Ok(())
                })))
            }
            1 => {
                self.move_to(at(0.5, 0.5));
                self.key(Key::N, Modifiers::COMMAND);
                Some(("Ctrl+N creates an empty graph", 0.3, Box::new(|app: &FerrotexApp| {
                    if app.graph.nodes.is_empty() { Ok(()) } else { Err(format!("{} nodes left", app.graph.nodes.len())) }
                })))
            }
            2 | 3 => {
                let (label, target) = if step == 2 { ("Noise", at(0.2, 0.3)) } else { ("Blur", at(0.55, 0.3)) };
                let Some(from) = lib(label) else { return Some(("drag from library", 0.0, fail(format!("no library item {label}")))) };
                self.drag(from, target);
                let kind = if step == 2 { NodeKind::Noise } else { NodeKind::Blur };
                let name = if step == 2 { "drag Noise from the library onto the graph" } else { "drag Blur from the library onto the graph" };
                Some((name, 0.3, Box::new(move |app: &FerrotexApp| {
                    if app.graph.nodes.iter().any(|n| n.kind == kind) { Ok(()) } else { Err("node not created".into()) }
                })))
            }
            4 => {
                let (Some(noise), Some(blur)) = (node_of(NodeKind::Noise), node_of(NodeKind::Blur)) else {
                    return Some(("connect sockets", 0.0, fail("nodes missing".into())));
                };
                let (Some(a), Some(b)) = (socket(noise, "o:output", true), socket(blur, "i:input", false)) else {
                    return Some(("connect sockets", 0.0, fail("sockets not found".into())));
                };
                self.drag(a, b);
                Some(("drag from an output socket to an input socket connects them", 0.3, Box::new(move |app: &FerrotexApp| {
                    match app.graph.link_into(blur, "i:input") {
                        Some(l) if l.from == noise => Ok(()),
                        _ => Err("no link Noise -> Blur".into()),
                    }
                })))
            }
            5 => {
                let blur = node_of(NodeKind::Blur)?;
                let h = header(blur)?;
                self.click(h.center());
                Some(("clicking a node selects it", 0.2, Box::new(move |app: &FerrotexApp| {
                    if app.view.selected.len() == 1 && app.view.selected.contains(&blur) { Ok(()) } else { Err(format!("selection {:?}", app.view.selected)) }
                })))
            }
            6 => {
                let blur = node_of(NodeKind::Blur)?;
                let h = header(blur)?;
                let before = app.graph.node(blur)?.pos;
                let zoom = app.view.zoom;
                self.drag(h.center(), h.center() + vec2(90.0, 70.0));
                Some(("dragging a node moves it", 0.2, Box::new(move |app: &FerrotexApp| {
                    let after = app.graph.node(blur).ok_or("node gone")?.pos;
                    let (dx, dy) = ((after[0] - before[0]) * zoom, (after[1] - before[1]) * zoom);
                    if (dx - 90.0).abs() < 12.0 && (dy - 70.0).abs() < 12.0 { Ok(()) } else { Err(format!("moved by ({dx:.1}, {dy:.1}) px")) }
                })))
            }
            7 => {
                let blur = node_of(NodeKind::Blur)?;
                let r = body(blur)?;
                self.double_click(r.center());
                Some(("double-click shows the node in the 2D view", 0.5, Box::new(move |app: &FerrotexApp| {
                    if app.preview.node == Some(blur) { Ok(()) } else { Err(format!("preview is {:?}", app.preview.node)) }
                })))
            }
            8 => {
                let blur = node_of(NodeKind::Blur)?;
                let Some(r) = app.props.debug_rects.get("expose:radius").copied() else {
                    return Some(("expose toggle", 0.0, fail("no expose toggle for radius".into())));
                };
                self.click(r.center());
                Some(("expose toggle adds an input socket to the node", 0.3, Box::new(move |app: &FerrotexApp| {
                    let node = app.graph.node(blur).ok_or("node gone")?;
                    if app.graph.input_sockets(node).iter().any(|s| s.key == "p:radius") { Ok(()) } else { Err("no p:radius socket".into()) }
                })))
            }
            9 => {
                self.move_to(at(0.3, 0.75));
                self.key(Key::Space, Modifiers::NONE);
                self.text("levels");
                self.key(Key::Enter, Modifiers::NONE);
                Some(("Space opens the node search and Enter adds the node", 0.3, Box::new(|app: &FerrotexApp| {
                    if app.graph.nodes.iter().any(|n| n.kind == NodeKind::Levels) { Ok(()) } else { Err("no Levels node".into()) }
                })))
            }
            10 => {
                let blur = node_of(NodeKind::Blur)?;
                let out = socket(blur, "o:output", true)?;
                self.drag(out, at(0.85, 0.7));
                self.text("invert");
                self.key(Key::Enter, Modifiers::NONE);
                Some(("dropping a link on empty space adds and connects a node", 0.3, Box::new(move |app: &FerrotexApp| {
                    let inv = app.graph.nodes.iter().find(|n| n.kind == NodeKind::Invert).ok_or("no Invert node")?;
                    match app.graph.link_into(inv.id, "i:input") {
                        Some(l) if l.from == blur => Ok(()),
                        _ => Err("Invert is not connected to Blur".into()),
                    }
                })))
            }
            11 => {
                let from = lib("Math")?;
                self.drag(from, at(0.2, 0.65));
                Some(("drag Math from the library", 0.4, Box::new(|app: &FerrotexApp| {
                    let m = app.graph.nodes.iter().find(|n| n.kind == NodeKind::Math).ok_or("no Math node")?;
                    if app.view.selected.contains(&m.id) { Ok(()) } else { Err("new node is not selected".into()) }
                })))
            }
            12 => {
                let Some(r) = app.props.debug_rects.get("expr").copied() else {
                    return Some(("type expression", 0.0, fail("expression editor not visible".into())));
                };
                self.click(r.center());
                self.key(Key::A, Modifiers::COMMAND);
                self.text("sin(x");
                Some(("an invalid expression reports an error instead of crashing", 0.4, Box::new(|app: &FerrotexApp| {
                    let m = app.graph.nodes.iter().find(|n| n.kind == NodeKind::Math).ok_or("no Math node")?;
                    if m.text("expression") != "sin(x" {
                        return Err(format!("expression is '{}'", m.text("expression")));
                    }
                    match app.engine.state(m.id).and_then(|s| s.error.clone()) {
                        Some(e) if e.contains("incorrect") => Ok(()),
                        other => Err(format!("unexpected state {other:?}")),
                    }
                })))
            }
            13 => {
                let r = app.props.debug_rects.get("expr").copied()?;
                self.click(r.center());
                self.key(Key::A, Modifiers::COMMAND);
                self.text("(1+x)*sin(myVar)-2.4/cos(myVar2)");
                Some(("typing the example expression creates three variable sockets", 0.4, Box::new(|app: &FerrotexApp| {
                    let m = app.graph.nodes.iter().find(|n| n.kind == NodeKind::Math).ok_or("no Math node")?;
                    let keys: Vec<String> = app.graph.input_sockets(m).into_iter().map(|s| s.key).collect();
                    if keys != ["v:x", "v:myVar", "v:myVar2"] {
                        return Err(format!("sockets {keys:?}"));
                    }
                    match app.engine.value(m.id) {
                        Some(crate::graph::Value::Float(v)) if (v + 2.4).abs() < 1e-4 => Ok(()),
                        other => Err(format!("value {other:?}, expected -2.4")),
                    }
                })))
            }
            14 => {
                let (math, blur) = (node_of(NodeKind::Math)?, node_of(NodeKind::Blur)?);
                let (a, b) = (socket(math, "o:result", true)?, socket(blur, "p:radius", false)?);
                self.drag(a, b);
                Some(("a Math result drives an exposed parameter", 0.4, Box::new(move |app: &FerrotexApp| {
                    if app.graph.link_into(blur, "p:radius").is_none() {
                        return Err("no link into radius".into());
                    }
                    match app.engine.state(blur).and_then(|s| s.driven.get("radius").cloned()) {
                        Some(v) if (v.as_f32() + 2.4).abs() < 1e-4 => Ok(()),
                        other => Err(format!("driven radius {other:?}")),
                    }
                })))
            }
            15 => {
                let (noise, math) = (node_of(NodeKind::Noise)?, node_of(NodeKind::Math)?);
                let (a, b) = (socket(noise, "o:output", true)?, socket(math, "v:x", false)?);
                self.drag(a, b);
                Some(("an image cannot be connected to a float input", 0.3, Box::new(move |app: &FerrotexApp| {
                    if app.graph.link_into(math, "v:x").is_some() {
                        return Err("incompatible link was created".into());
                    }
                    match &app.status {
                        Some(s) if s.error && s.text.contains("Cannot connect") => Ok(()),
                        _ => Err("no error message shown".into()),
                    }
                })))
            }
            16 => {
                // Box select from an empty corner over Noise and Blur.
                let (noise, blur) = (node_of(NodeKind::Noise)?, node_of(NodeKind::Blur)?);
                let (rn, rb) = (body(noise)?, body(blur)?);
                let area = rn.union(rb);
                let start = area.min - vec2(25.0, 25.0);
                self.drag(start, area.max + vec2(10.0, 10.0));
                Some(("box selection selects the covered nodes", 0.3, Box::new(move |app: &FerrotexApp| {
                    if app.view.selected.contains(&noise) && app.view.selected.contains(&blur) { Ok(()) } else { Err(format!("selection {:?}", app.view.selected)) }
                })))
            }
            17 => {
                self.move_to(at(0.95, 0.95));
                self.key(Key::Delete, Modifiers::NONE);
                Some(("Delete removes the selected nodes and their links", 0.9, Box::new(|app: &FerrotexApp| {
                    let left = app.graph.nodes.iter().filter(|n| matches!(n.kind, NodeKind::Noise | NodeKind::Blur)).count();
                    let dangling = app.graph.links.iter().any(|l| !app.graph.contains(l.from) || !app.graph.contains(l.to));
                    if left == 0 && !dangling { Ok(()) } else { Err(format!("{left} nodes left, dangling links: {dangling}")) }
                })))
            }
            18 => {
                self.key(Key::Z, Modifiers::COMMAND);
                Some(("Ctrl+Z restores the deleted nodes and links", 0.9, Box::new(|app: &FerrotexApp| {
                    let blur = app.graph.nodes.iter().find(|n| n.kind == NodeKind::Blur).ok_or("Blur not restored")?;
                    let noise = app.graph.nodes.iter().find(|n| n.kind == NodeKind::Noise).ok_or("Noise not restored")?;
                    match app.graph.link_into(blur.id, "i:input") {
                        Some(l) if l.from == noise.id => Ok(()),
                        _ => Err("link not restored".into()),
                    }
                })))
            }
            19 => {
                let add = app.inputs.debug_rects.get("add").copied()?;
                self.click(add.center());
                Some(("open the '+ Add' input menu", 0.3, Box::new(|app: &FerrotexApp| {
                    if app.inputs.debug_rects.contains_key("add:Float") { Ok(()) } else { Err("menu did not open".into()) }
                })))
            }
            20 => {
                let item = app.inputs.debug_rects.get("add:Float").copied()?;
                self.click(item.center());
                Some(("adding a Float graph input", 0.3, Box::new(|app: &FerrotexApp| {
                    if app.graph.inputs.iter().any(|i| i.ty == crate::graph::DataType::Float) { Ok(()) } else { Err("no input".into()) }
                })))
            }
            21 => {
                // Right click on empty canvas opens the context menu (it must not pan or crash).
                let p = at(0.9, 0.12);
                self.click_with(p, PointerButton::Secondary);
                self.key(Key::Escape, Modifiers::NONE);
                Some(("context menu on the canvas", 0.3, Box::new(|app: &FerrotexApp| {
                    if app.engine.gpu.errors.lock().map(|e| e.is_empty()).unwrap_or(false) { Ok(()) } else { Err("GPU errors reported".into()) }
                })))
            }
            22 => {
                self.move_to(at(0.5, 0.5));
                for _ in 0..3 {
                    self.frames.push_back(vec![Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Line,
                        delta: vec2(0.0, 2.0),
                        phase: egui::TouchPhase::Move,
                        modifiers: Modifiers::NONE,
                    }]);
                }
                self.idle(2);
                let zoom = app.view.zoom;
                Some(("mouse wheel zooms the graph", 0.2, Box::new(move |app: &FerrotexApp| {
                    if app.view.zoom > zoom * 1.1 { Ok(()) } else { Err(format!("zoom {zoom} -> {}", app.view.zoom)) }
                })))
            }
            _ => None,
        }
    }
}

