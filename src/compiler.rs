use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

use crate::ast::{Attr, Chunk, File, Node, RawValue, Section, Target};
use crate::data::{parse_kdl, Value};

pub struct Compiler {
    pub root: PathBuf,
    pub files: HashMap<String, File>,
}

pub struct Render<'c> {
    comp: &'c Compiler,
    bundle: Bundle,
    stack: Vec<String>,
    data_cache: HashMap<String, Value>,
}

struct Bundle {
    css: Vec<String>,
    js: Vec<String>,
    seen: HashSet<(String, String)>,
}

impl Bundle {
    fn new() -> Bundle {
        Bundle {
            css: Vec::new(),
            js: Vec::new(),
            seen: HashSet::new(),
        }
    }

    fn add(&mut self, file: &str, section: &str, ext: &str, text: &str) {
        if !self.seen.insert((file.to_string(), section.to_string())) {
            return;
        }
        if ext == "css" {
            self.css.push(text.to_string());
        } else {
            self.js.push(text.to_string());
        }
    }
}

#[derive(Default, Clone)]
struct Env {
    vars: HashMap<String, Value>,
}

pub fn resolve_file_key(base_dir: &str, reference: &str) -> String {
    if base_dir.is_empty() {
        return normalize(reference);
    }
    normalize(&format!("{base_dir}/{reference}"))
}

fn normalize(p: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for part in p.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s.to_string()),
        }
    }
    parts.join("/")
}

fn strip_braces(s: &str) -> &str {
    let s = s.trim();
    if s.starts_with('{') && s.ends_with('}') {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

fn is_file_ref(s: &str) -> bool {
    s.starts_with("./") || s.starts_with("../") || s.starts_with('/')
}

#[derive(Debug, Clone)]
enum Seg {
    Field(String),
    Index(i64),
}

/// Split a non-file reference into its root identifier and traversal segments.
/// `item.name` → ("item", [Field("name")]); `item[2].x` → ("item", [Index(2), Field("x")]).
fn split_segments(expr: &str) -> (String, Vec<Seg>) {
    let mut root_end = expr.len();
    for (i, c) in expr.char_indices() {
        if c == '.' || c == '[' {
            root_end = i;
            break;
        }
    }
    (expr[..root_end].to_string(), parse_trailing(&expr[root_end..]))
}

/// Split a data-file reference into its file path (up to `.kdl`) and segments.
/// `./data.kdl.foo[2]` → ("./data.kdl", [Field("foo"), Index(2)]).
fn split_file_ref(expr: &str) -> Result<(String, Vec<Seg>), String> {
    let kdl = ".kdl";
    let pos = expr
        .find(kdl)
        .ok_or_else(|| format!("data file reference `{expr}` must reference a `.kdl` file"))?;
    let end = pos + kdl.len();
    Ok((expr[..end].to_string(), parse_trailing(&expr[end..])))
}

fn parse_trailing(s: &str) -> Vec<Seg> {
    let mut segs = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            '.' => {
                chars.next();
                let mut name = String::new();
                while let Some(&c2) = chars.peek() {
                    if c2 == '.' || c2 == '[' {
                        break;
                    }
                    name.push(c2);
                    chars.next();
                }
                segs.push(Seg::Field(name));
            }
            '[' => {
                chars.next();
                let mut inner = String::new();
                for c2 in chars.by_ref() {
                    if c2 == ']' {
                        break;
                    }
                    inner.push(c2);
                }
                let inner = inner.trim();
                if let Ok(n) = inner.parse::<i64>() {
                    segs.push(Seg::Index(n));
                } else {
                    let key = inner.trim_matches(['"', '\'']);
                    segs.push(Seg::Field(key.to_string()));
                }
            }
            _ => {
                chars.next();
            }
        }
    }
    segs
}

impl<'c> Render<'c> {
    pub fn new(comp: &'c Compiler) -> Render<'c> {
        Render {
            comp,
            bundle: Bundle::new(),
            stack: Vec::new(),
            data_cache: HashMap::new(),
        }
    }

    pub fn render_page(&mut self, key: &str) -> Result<String, String> {
        let file = self
            .comp
            .files
            .get(key)
            .ok_or_else(|| format!("page file `{key}` not found"))?;
        self.add_file_assets(file, key);

        let mut body = String::new();
        let env = Env::default();
        self.render_nodes(&file.main, &env, key, &mut body)?;

        let css = self.bundle.css.join("\n");
        let js = self.bundle.js.join("\n");
        Ok(wrap_document(&body, &css, &js))
    }

    fn add_file_assets(&mut self, file: &File, key: &str) {
        for (name, section) in &file.sections {
            if let Section::Asset { ext, text, .. } = section {
                self.bundle.add(key, name, ext, text);
            }
        }
    }

    fn guard(&mut self, id: &str) -> Result<(), String> {
        if self.stack.iter().any(|s| s == id) {
            let chain = self.stack.join(" > ");
            return Err(format!("recursive component invocation detected: {chain} > {id}"));
        }
        self.stack.push(id.to_string());
        Ok(())
    }

    fn render_nodes(
        &mut self,
        nodes: &[Node],
        env: &Env,
        file_key: &str,
        out: &mut String,
    ) -> Result<(), String> {
        for node in nodes {
            self.render_node(node, env, file_key, out)?;
        }
        Ok(())
    }

    fn render_node(
        &mut self,
        node: &Node,
        env: &Env,
        file_key: &str,
        out: &mut String,
    ) -> Result<(), String> {
        match node {
            Node::El(n) => {
                if n.tag == "br" {
                    out.push_str("<br/>");
                    return Ok(());
                }
                out.push('<');
                out.push_str(&n.tag);
                self.render_attrs(&n.attrs, env, file_key, out)?;
                out.push('>');
                self.render_nodes(&n.children, env, file_key, out)?;
                out.push_str("</");
                out.push_str(&n.tag);
                out.push('>');
                Ok(())
            }
            Node::Inv(n) => {
                if !n.children.is_empty() {
                    return Err(format!(
                        "component `{}` does not accept child content",
                        target_label(&n.target)
                    ));
                }
                self.expand_target(&n.target, &n.props, env, file_key, out)
            }
            Node::Cond(n) => {
                let cond = self.resolve_ref(&n.cond, env, file_key)?;
                if cond.truthy() {
                    self.render_nodes(&n.then_ch, env, file_key, out)
                } else {
                    self.render_nodes(&n.else_ch, env, file_key, out)
                }
            }
            Node::Loop(n) => {
                let value = self.resolve_ref(&n.expr, env, file_key)?;
                let items: Vec<Value> = match &value {
                    Value::Lst(l) => l.clone(),
                    other if other.truthy() => vec![other.clone()],
                    _ => Vec::new(),
                };
                for item in items {
                    let mut env2 = env.clone();
                    env2
                        .vars
                        .insert(n.var.clone(), item.clone());
                    self.render_nodes(&n.children, &env2, file_key, out)?;
                }
                Ok(())
            }
            Node::Text(n) => {
                for chunk in &n.chunks {
                    match chunk {
                        Chunk::Lit(s) => out.push_str(s),
                        Chunk::Ref(expr) => {
                            let v = self.resolve_ref(expr, env, file_key)?;
                            out.push_str(&v.to_text());
                        }
                    }
                }
                Ok(())
            }
            Node::ElseMarker(_) => Err("internal: unexpanded `<:else` node reached the renderer".into()),
        }
    }

    fn render_attrs(
        &mut self,
        attrs: &[Attr],
        env: &Env,
        file_key: &str,
        out: &mut String,
    ) -> Result<(), String> {
        for attr in attrs {
            out.push(' ');
            out.push_str(&attr.key);
            if let Some(v) = &attr.value {
                let value = self.resolve_raw(v, env, file_key)?;
                out.push_str("=\"");
                out.push_str(&value.to_text());
                out.push('"');
            }
        }
        Ok(())
    }

    fn resolve_raw(
        &mut self,
        raw: &RawValue,
        env: &Env,
        file_key: &str,
    ) -> Result<Value, String> {
        match raw {
            RawValue::Bool(b) => Ok(Value::Bool(*b)),
            RawValue::Num(n) => Ok(Value::Num(*n)),
            RawValue::Null => Ok(Value::Null),
            RawValue::Str(chunks) => {
                let mut s = String::new();
                for chunk in chunks {
                    match chunk {
                        Chunk::Lit(t) => s.push_str(t),
                        Chunk::Ref(expr) => {
                            let v = self.resolve_ref(expr, env, file_key)?;
                            s.push_str(&v.to_text());
                        }
                    }
                }
                Ok(Value::Str(s))
            }
            RawValue::Ref(expr) => self.resolve_ref(expr, env, file_key),
        }
    }

    fn resolve_ref(
        &mut self,
        expr: &str,
        env: &Env,
        file_key: &str,
    ) -> Result<Value, String> {
        let expr = strip_braces(expr);
        if expr.is_empty() {
            return Err("empty reference expression".into());
        }

        let (base, segments) = if is_file_ref(expr) {
            let (path, segs) = split_file_ref(expr)?;
            (self.load_data(file_key, &path)?, segs)
        } else {
            let (root, segs) = split_segments(expr);
            let base = match env.vars.get(&root) {
                Some(v) => v.clone(),
                None => {
                    let file = self
                        .comp
                        .files
                        .get(file_key)
                        .ok_or_else(|| format!("unknown file `{file_key}`"))?;
                    match file.sections.get(&root) {
                        Some(section) => self.section_value(file, file_key, &root, section)?,
                        None => {
                            return Err(format!("unresolved reference `{expr}`"));
                        }
                    }
                }
            };
            (base, segs)
        };

        let mut value = base;
        for seg in &segments {
            value = match seg {
                Seg::Field(name) => value
                    .get(name)
                    .ok_or_else(|| format!("reference `{expr}`: no field `{name}`"))?,
                Seg::Index(n) => value
                    .index_num(*n)
                    .ok_or_else(|| format!("reference `{expr}`: index `{n}` out of bounds"))?,
            };
        }
        Ok(value.collapse_args())
    }

fn section_value(
        &mut self,
        _file: &File,
        file_key: &str,
        name: &str,
        section: &Section,
    ) -> Result<Value, String> {
        match section {
            Section::Asset { text, .. } => Ok(Value::Str(text.clone())),
            Section::Alias(target) => {
                if let Target::File { path, section: None } = target {
                    if path.ends_with(".kdl") {
                        // an alias to a data file binds the parsed KDL value,
                        // so `{name.field}` traversal works
                        return self.load_data(file_key, path);
                    }
                }
                let mut tmp = String::new();
                let env = Env::default();
                self.expand_target_rec(file_key, target, &[], &env, &mut tmp)?;
                Ok(Value::Str(tmp))
            }
            Section::Component(body) => {
                let mut tmp = String::new();
                let env = Env::default();
                self.render_component_body(file_key, name, body, &env, &mut tmp)?;
                Ok(Value::Str(tmp))
            }
        }
    }

    fn render_component_body(
        &mut self,
        file_key: &str,
        name: &str,
        body: &[Node],
        env: &Env,
        out: &mut String,
    ) -> Result<(), String> {
        let id = format!("{file_key}#{name}");
        self.guard(&id)?;
        if let Some(file) = self.comp.files.get(file_key) {
            self.add_file_assets(file, file_key);
        }
        self.render_nodes(body, env, file_key, out)?;
        self.stack.pop();
        Ok(())
    }

    fn expand_target(
        &mut self,
        target: &Target,
        props: &[Attr],
        env: &Env,
        file_key: &str,
        out: &mut String,
    ) -> Result<(), String> {
        self.expand_target_rec(file_key, target, props, env, out)
    }

    fn expand_target_rec(
        &mut self,
        file_key: &str,
        target: &Target,
        props: &[Attr],
        env: &Env,
        out: &mut String,
    ) -> Result<(), String> {
        match target {
            Target::Local(name) => self.invoke_component(file_key, name, props, env, out),
            Target::Var(name) => {
                let file = self
                    .comp
                    .files
                    .get(file_key)
                    .ok_or_else(|| format!("unknown file `{file_key}`"))?;
                match file.sections.get(name) {
                    Some(Section::Alias(sub_target)) => {
                        let id = format!("{file_key}#{name}");
                        self.guard(&id)?;
                        let r = self.expand_target_rec(file_key, sub_target, props, env, out);
                        self.stack.pop();
                        r
                    }
                    Some(Section::Component(_)) => self.invoke_component(file_key, name, props, env, out),
                    Some(Section::Asset { ext, text, .. }) => {
                        self.bundle.add(file_key, name, ext, text);
                        Ok(())
                    }
                    None => Err(format!("unresolved alias `{{{name}}}` in `{file_key}`")),
                }
            }
            Target::File { path, section } => {
                let key = {
                    let base_dir = file_key
                        .rsplit_once('/')
                        .map(|(d, _)| d)
                        .unwrap_or("");
                    resolve_file_key(base_dir, path)
                };
                let home_file = self.comp.files.get(&key);
                match home_file {
                    Some(_) => {
                        let sec = section
                            .as_deref()
                            .ok_or_else(|| format!("file reference `{path}` needs a `#section`"))?;
                        self.invoke_component(&key, sec, props, env, out)
                    }
                    None if path.ends_with(".df") => {
                        return Err(format!(
                            "referenced Dreamfish file `{path}` does not exist (expected `{key}`)"
                        ));
                    }
                    None => {
                        let ext = if path.ends_with(".css") {
                            "css"
                        } else if path.ends_with(".js") {
                            "js"
                        } else {
                            return Err(format!("cannot include raw asset `{path}`: unsupported extension"));
                        };
                        let text = self.read_asset(&key)?;
                        self.bundle.add(&key, path, ext, &text);
                        Ok(())
                    }
                }
            }
        }
    }

    fn invoke_component(
        &mut self,
        file_key: &str,
        name: &str,
        props: &[Attr],
        env: &Env,
        out: &mut String,
    ) -> Result<(), String> {
        let file = self
            .comp
            .files
            .get(file_key)
            .ok_or_else(|| format!("unknown file `{file_key}`"))?;
        let section = file
            .sections
            .get(name)
            .ok_or_else(|| format!("unknown component `#{name}` in `{file_key}`"))?;
        match section {
            Section::Component(body) => {
                let mut env2 = Env::default();
                for attr in props {
                    let value = self.resolve_raw(&attr.value.clone().unwrap_or(RawValue::Bool(true)), env, file_key)?;
                    if env2.vars.insert(attr.key.clone(), value).is_some() {
                        return Err(format!("duplicate prop `{}` on component `#{name}`", attr.key));
                    }
                }
                self.render_component_body(file_key, name, body, &env2, out)
            }
            Section::Alias(sub_target) => {
                let id = format!("{file_key}#{name}");
                self.guard(&id)?;
                let r = self.expand_target_rec(file_key, sub_target, props, env, out);
                self.stack.pop();
                r
            }
            Section::Asset { ext, text, .. } => {
                self.bundle.add(file_key, name, ext, text);
                Ok(())
            }
        }
    }

    fn load_data(&mut self, file_key: &str, reference: &str) -> Result<Value, String> {
        let base_dir = file_key.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        let key = resolve_file_key(base_dir, reference);
        let cache_key = format!("{file_key}::{reference}");
        if let Some(v) = self.data_cache.get(&cache_key) {
            return Ok(v.clone());
        }
        let path = self.comp.root.join(&key);
        let src =
            fs::read_to_string(&path).map_err(|e| format!("cannot load data file `{key}`: {e}"))?;
        let value = parse_kdl(&src).map_err(|e| format!("error parsing `{key}`: {e}"))?;
        self.data_cache.insert(cache_key, value.clone());
        Ok(value)
    }

    fn read_asset(&mut self, key: &str) -> Result<String, String> {
        let path = self.comp.root.join(key);
        fs::read_to_string(&path).map_err(|e| format!("cannot load asset `{key}`: {e}"))
    }
}

fn wrap_document(body: &str, css: &str, js: &str) -> String {
    let mut out = String::new();
    out.push_str("<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n");
    if !css.is_empty() {
        out.push_str("<style>\n");
        out.push_str(css);
        out.push_str("\n</style>\n");
    }
    out.push_str("</head>\n<body>\n");
    out.push_str(body);
    out.push('\n');
    if !js.is_empty() {
        out.push_str("<script>\n");
        out.push_str(js);
        out.push_str("\n</script>\n");
    }
    out.push_str("</body>\n</html>\n");
    out
}

pub fn target_label(t: &Target) -> String {
    match t {
        Target::Local(n) => format!("#{n}"),
        Target::Var(n) => format!("{{{n}}}"),
        Target::File { path, section } => match section {
            Some(s) => format!("{path}#{s}"),
            None => path.clone(),
        },
    }
}

pub fn page_route(key: &str) -> String {
    if let Some(stripped) = key.strip_suffix(".df") {
        format!("{stripped}.html")
    } else {
        key.to_string()
    }
}