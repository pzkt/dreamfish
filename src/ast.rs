use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct File {
    pub main: Vec<Node>,
    pub sections: BTreeMap<String, Section>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum Section {
    Component(Vec<Node>),
    Alias(Target),
    Asset { name: String, ext: String, text: String },
}

#[derive(Debug, Clone)]
pub enum Node {
    El(ElNode),
    Inv(InvNode),
    Cond(CondNode),
    Loop(LoopNode),
    Text(TextNode),
    ElseMarker(ElseStub),
}

#[derive(Debug, Clone)]
pub struct ElseStub {
    pub line: usize,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone)]
pub struct ElNode {
    pub line: usize,
    pub tag: String,
    pub attrs: Vec<Attr>,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone)]
pub struct InvNode {
    pub line: usize,
    pub target: Target,
    pub props: Vec<Attr>,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone)]
pub struct CondNode {
    pub line: usize,
    pub cond: String,
    pub then_ch: Vec<Node>,
    pub else_ch: Vec<Node>,
}

#[derive(Debug, Clone)]
pub struct LoopNode {
    pub line: usize,
    pub var: String,
    pub expr: String,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone)]
pub struct TextNode {
    pub line: usize,
    pub chunks: Vec<Chunk>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Local(String),
    Var(String),
    File { path: String, section: Option<String> },
}

#[derive(Debug, Clone)]
pub struct Attr {
    pub key: String,
    pub value: Option<RawValue>,
}

#[derive(Debug, Clone)]
pub enum RawValue {
    Str(Vec<Chunk>),
    Bool(bool),
    Null,
    Num(f64),
    Ref(String),
}

#[derive(Debug, Clone)]
pub enum Chunk {
    Lit(String),
    Ref(String),
}

impl Node {
    pub fn children_mut(&mut self) -> Option<&mut Vec<Node>> {
        match self {
            Node::El(n) => Some(&mut n.children),
            Node::Inv(n) => Some(&mut n.children),
            Node::Cond(n) => Some(&mut n.then_ch),
            Node::Loop(n) => Some(&mut n.children),
            Node::ElseMarker(n) => Some(&mut n.children),
            Node::Text(_) => None,
        }
    }

    pub fn line(&self) -> usize {
        match self {
            Node::El(n) => n.line,
            Node::Inv(n) => n.line,
            Node::Cond(n) => n.line,
            Node::Loop(n) => n.line,
            Node::Text(n) => n.line,
            Node::ElseMarker(n) => n.line,
        }
    }
}