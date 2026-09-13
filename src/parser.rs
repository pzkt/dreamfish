use std::collections::BTreeMap;

use crate::ast::{Node, Section, Target, File};
use crate::lexer::{lex, Tok};

#[derive(Default)]
struct Frame {
    parts: Vec<Node>,
    sink: Vec<Node>,
    else_ref: Option<ElseRef>,
}

#[derive(Clone, Copy)]
enum ElseRef {
    Container(usize),
    Level { depth: usize, idx: usize },
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Main,
    Comp,
    None,
}

pub struct Parser {
    container: Vec<Node>,
    levels: Vec<Frame>,
    depth: usize,
    main: Vec<Node>,
    sections: BTreeMap<String, Section>,
    phase: Phase,
    current_comp: Option<String>,
    pending_asset: Option<String>,
    main_done: bool,
}

impl Parser {
    fn new() -> Parser {
        Parser {
            container: Vec::new(),
            levels: vec![Frame::default()],
            depth: 0,
            main: Vec::new(),
            sections: BTreeMap::new(),
            phase: Phase::Main,
            current_comp: None,
            pending_asset: None,
            main_done: false,
        }
    }

    fn indent(&mut self) {
        self.depth += 1;
        self.levels.push(Frame::default());
    }

    fn dedent(&mut self, line: usize) -> Result<(), String> {
        if self.depth == 0 {
            return Err(format!("line {line}: unexpected dedent"));
        }
        self.finalize_level(self.depth)?;
        self.depth -= 1;
        self.levels.pop();
        Ok(())
    }

    fn chain(&mut self, nodes: Vec<Node>, line: usize) -> Result<(), String> {
        if self.phase == Phase::None {
            return Err(format!("line {line}: content is not inside a section body or main template"));
        }
        if self.depth > 0
            && (self.levels.get(self.depth - 1).map(|f| f.parts.is_empty()).unwrap_or(true))
        {
            return Err(format!("line {line}: unindented content has no parent node"));
        }

        self.finalize_level(self.depth)?;

        let is_else = matches!(nodes.first(), Some(Node::ElseMarker(_)));

        if is_else {
            let idx = if self.depth == 0 {
                self.container.len().checked_sub(1)
            } else {
                self.levels[self.depth - 1].sink.len().checked_sub(1)
            };
            let idx = match idx {
                Some(i) => i,
                None => {
                    return Err(format!("line {line}: `<:else` without a matching `<:if`"))
                }
            };
            let cond = if self.depth == 0 {
                self.container.get_mut(idx)
            } else {
                self.levels[self.depth - 1].sink.get_mut(idx)
            };
            match cond {
                Some(Node::Cond(c)) if c.else_ch.is_empty() => {}
                _ => return Err(format!("line {line}: `<:else` without a matching `<:if`")),
            }
            let elseref = if self.depth == 0 {
                ElseRef::Container(idx)
            } else {
                ElseRef::Level {
                    depth: self.depth - 1,
                    idx,
                }
            };
            self.levels[self.depth] = Frame {
                parts: nodes,
                sink: Vec::new(),
                else_ref: Some(elseref),
            };
            return Ok(());
        }

        self.levels[self.depth] = Frame {
            parts: nodes,
            sink: Vec::new(),
            else_ref: None,
        };
        Ok(())
    }

    fn finalize_level(&mut self, d: usize) -> Result<(), String> {
        if d >= self.levels.len() {
            return Ok(());
        }
        let frame = std::mem::take(&mut self.levels[d]);
        if frame.parts.is_empty() {
            return Ok(());
        }
        if let Some(eref) = frame.else_ref {
            let children = if frame.parts.len() == 1 {
                frame.sink
            } else {
                let root = finish(frame.parts, frame.sink)?;
                match root {
                    Node::ElseMarker(stub) => stub.children,
                    _ => return Err("internal: `<:else` chain did not root in an else node".into()),
                }
            };
            match eref {
                ElseRef::Container(idx) => {
                    match self.container.get_mut(idx) {
                        Some(Node::Cond(c)) => c.else_ch.extend(children),
                        _ => return Err("internal: else target is not an `<:if`".into()),
                    }
                }
                ElseRef::Level { depth, idx } => match self.levels[depth].sink.get_mut(idx) {
                    Some(Node::Cond(c)) => c.else_ch.extend(children),
                    _ => return Err("internal: else target is not an `<:if`".into()),
                },
            }
        } else {
            let root = finish(frame.parts, frame.sink)?;
            if d == 0 {
                self.container.push(root);
            } else {
                self.levels[d - 1].sink.push(root);
            }
        }
        Ok(())
    }

    fn section(&mut self, name: String, alias: Option<Target>, line: usize) -> Result<(), String> {
        for d in (0..=self.depth).rev() {
            self.finalize_level(d)?;
        }
        self.depth = 0;
        self.levels = vec![Frame::default()];

        self.close_container()?;

        if self.sections.contains_key(&name) {
            return Err(format!("line {line}: duplicate section name `#{name}`"));
        }

        if let Some(target) = alias {
            self.sections.insert(name, Section::Alias(target));
            self.phase = Phase::None;
            self.current_comp = None;
            return Ok(());
        }

        if name.ends_with(".css") || name.ends_with(".js") {
            self.pending_asset = Some(name);
            self.phase = Phase::None;
            self.current_comp = None;
            return Ok(());
        }

        self.current_comp = Some(name);
        self.phase = Phase::Comp;
        Ok(())
    }

    fn raw(&mut self, text: String, line: usize) -> Result<(), String> {
        if let Some(name) = self.pending_asset.take() {
            let ext = if name.ends_with(".css") {
                "css".to_string()
            } else {
                "js".to_string()
            };
            self.sections.insert(
                name.clone(),
                Section::Asset {
                    name,
                    ext,
                    text: text.trim().to_string(),
                },
            );
            return Ok(());
        }
        Err(format!("line {line}: unexpected raw content: asset section has no pending header"))
    }

    fn close_container(&mut self) -> Result<(), String> {
        match self.phase {
            Phase::Main => {
                self.main = std::mem::take(&mut self.container);
                self.main_done = true;
            }
            Phase::Comp => {
                if let Some(name) = self.current_comp.take() {
                    let body = std::mem::take(&mut self.container);
                    self.sections.insert(name, Section::Component(body));
                }
            }
            Phase::None => {
                if !self.container.is_empty() {
                    return Err("content found between sections".into());
                }
            }
        }
        Ok(())
    }

    fn finish(mut self) -> Result<File, String> {
        for d in (0..=self.depth).rev() {
            self.finalize_level(d)?;
        }
        self.close_container()?;
        if !self.main_done {
            self.main = std::mem::take(&mut self.container);
        }
        if let Some(name) = self.pending_asset.take() {
            return Err(format!("asset section `#{name}` has no body"));
        }
        Ok(File {
            main: self.main.clone(),
            sections: self.sections.clone(),
        })
    }
}

pub fn parse(path: &str, src: &str) -> Result<File, String> {
    let toks = lex(src).map_err(|e| format!("{}:{}: {}", path, e.line, e.msg))?;
    let mut p = Parser::new();
    for t in toks.iter() {
        match t {
            Tok::Indent => p.indent(),
            Tok::Dedent => p.dedent(0)?,
            Tok::Section { name, alias, line } => p.section(name.clone(), alias.clone(), *line)?,
            Tok::Chain { nodes, line } => p.chain(nodes.clone(), *line)?,
            Tok::Raw { text, line } => p.raw(text.clone(), *line)?,
        }
    }
    p.finish()
}

fn finish(parts: Vec<Node>, sink: Vec<Node>) -> Result<Node, String> {
    let mut parts = parts;
    let last = parts.pop().unwrap();
    let mut acc = attach_children(last, sink)?;
    for p in parts.into_iter().rev() {
        acc = attach_child(p, acc)?;
    }
    Ok(acc)
}

fn attach_child(parent: Node, child: Node) -> Result<Node, String> {
    match parent {
        Node::Text(_) => Err(format!("text node cannot contain child elements (parent={parent:?} child={child:?})")),
        mut other => {
            if let Some(ch) = other.children_mut() {
                ch.push(child);
            }
            Ok(other)
        }
    }
}

fn attach_children(parent: Node, children: Vec<Node>) -> Result<Node, String> {
    if children.is_empty() {
        return Ok(parent);
    }
    match parent {
        Node::Text(_) => Err(format!("text node cannot contain child elements (parent={parent:?} children={children:?})")),
        mut other => {
            if let Some(ch) = other.children_mut() {
                ch.extend(children);
            }
            Ok(other)
        }
    }
}