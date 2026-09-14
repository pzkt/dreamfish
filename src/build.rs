use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use clap::Args;

use crate::ast::{Attr, Chunk, File, Node, Section};
use crate::compiler::{page_route, Compiler, Render};
use crate::parser;

#[derive(Args, Clone)]
pub struct BuildArgs {
    /// Source directory containing templates
    #[arg(long, default_value = ".")]
    pub input: String,

    /// Directory to write the generated site into
    #[arg(long, default_value = ".dreamfish/build")]
    pub output: String,
}

pub fn run(args: BuildArgs) -> Result<(), String> {
    let input = PathBuf::from(&args.input);
    let output = PathBuf::from(&args.output);
    if !input.is_dir() {
        return Err(format!("input directory `{}` does not exist", input.display()));
    }
    clean_output(&output, &input)?;

    let mut files = BTreeMap::new();
    let mut pages: Vec<String> = Vec::new();
    collect_files(&input, &input, &output, &mut files, &mut pages)?;

    if files.is_empty() {
        return Err("no `.df` files found".into());
    }

    for (key, file) in &files {
        validate_collisions(key, file)?;
    }

    let compiler = Compiler {
        root: input.clone(),
        files: files.into_iter().collect(),
    };

    let mut built = 0usize;
    for key in &pages {
        let mut render = Render::new(&compiler);
        match render.render_page(key) {
            Ok(html) => {
                let route = page_route(key);
                let dest = output.join(&route);
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|e| format!("cannot create `{}`: {e}", parent.display()))?;
                }
                fs::write(&dest, html)
                    .map_err(|e| format!("cannot write `{}`: {e}", dest.display()))?;
                println!("{}", crate::term::gray(&format!("building `{key}` -> `{}`", dest.display())));
                built += 1;
            }
            Err(e) => return Err(format!("{key}: {e}")),
        }
    }

    println!(
        "{}",
        &format!(
            "{}: built {built} page{} from {} file{} into `{}`",
            crate::term::green("Complete"),
            if built > 1 { "s" } else { "" },
            compiler.files.len(),
            if compiler.files.len() > 1 { "s" } else { "" },
            output.display()
        )
    );
    Ok(())
}

fn clean_output(output: &Path, input: &Path) -> Result<(), String> {
    let input = abs_norm(input);
    let output = abs_norm(output);
    if output == input {
        return Err(format!(
            "output directory `{}` must not be the input directory",
            output.display()
        ));
    }
    if input.starts_with(&output) {
        return Err(format!(
            "output directory `{}` would erase the input directory `{}`",
            output.display(),
            input.display()
        ));
    }
    if output.exists() {
        fs::remove_dir_all(&output)
            .map_err(|e| format!("cannot clean output `{}`: {e}", output.display()))?;
    }
    fs::create_dir_all(&output)
        .map_err(|e| format!("cannot create `{}`: {e}", output.display()))
}

pub(crate) fn abs_norm(p: &Path) -> PathBuf {
    let abs = if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|c| c.join(p))
            .unwrap_or_else(|_| p.to_path_buf())
    };
    let mut norm = PathBuf::new();
    for part in abs.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                norm.pop();
            }
            other => norm.push(other.as_os_str()),
        }
    }
    norm
}

fn collect_files(
    root: &Path,
    dir: &Path,
    output: &Path,
    files: &mut BTreeMap<String, File>,
    pages: &mut Vec<String>,
) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("cannot read `{}`: {e}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        if path.is_dir() {
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            if path.starts_with(output) {
                continue;
            }
            collect_files(root, &path, output, files, pages)?;
        } else if name.ends_with(".df") {
            let rel = path
                .strip_prefix(root)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            let source = fs::read_to_string(&path)
                .map_err(|e| format!("cannot read `{}`: {e}", path.display()))?;
            let file = parser::parse(&rel, &source).map_err(|e| format!("parse error: {e}"))?;
            if !file.main.is_empty() {
                pages.push(rel.clone());
            }
            files.insert(rel, file);
        }
    }
    Ok(())
}

pub fn validate_collisions(key: &str, file: &File) -> Result<(), String> {
    for (name, r, _line) in find_collisions(key, file) {
        return Err(format!(
            "{key}: prop/variable `{{{r}}}` used inside component `#{name}` collides with a \
             section of the same name defined in this file"
        ));
    }
    Ok(())
}

/// Return every `(component name, colliding ref name, 1-based line)` pair for
/// a file. A prop/variable ref used inside a component body that equals a
/// section name of the same file is a collision error (§6.4 of SPEC.md).
pub fn find_collisions(_key: &str, file: &File) -> Vec<(String, String, usize)> {
    let mut out = Vec::new();
    for (name, section) in &file.sections {
        let body = match section {
            Section::Component(body) => body,
            _ => continue,
        };
        let mut refs = HashSet::new();
        collect_refs(body, &mut refs);
        for r in refs {
            if file.sections.contains_key(&r) {
                if let Some(line) = ref_line(body, &r) {
                    out.push((name.clone(), r, line));
                } else {
                    out.push((name.clone(), r, 0));
                }
            }
        }
    }
    out
}

fn ref_line(nodes: &[Node], wanted: &str) -> Option<usize> {
    for node in nodes {
        match node {
            Node::El(n) => {
                if n.attrs.iter().any(|a| attr_has_ref(a, wanted)) {
                    return Some(n.line);
                }
                if let Some(l) = ref_line(&n.children, wanted) {
                    return Some(l);
                }
            }
            Node::Inv(n) => {
                if n.props.iter().any(|a| attr_has_ref(a, wanted)) {
                    return Some(n.line);
                }
                if let Some(l) = ref_line(&n.children, wanted) {
                    return Some(l);
                }
            }
            Node::Cond(n) => {
                if ident_texts(&n.cond).iter().any(|t| t == wanted) {
                    return Some(n.line);
                }
                if let Some(l) = ref_line(&n.then_ch, wanted) {
                    return Some(l);
                }
                if let Some(l) = ref_line(&n.else_ch, wanted) {
                    return Some(l);
                }
            }
            Node::Loop(n) => {
                if ident_texts(&n.expr).iter().any(|t| t == wanted) {
                    return Some(n.line);
                }
                if let Some(l) = ref_line(&n.children, wanted) {
                    return Some(l);
                }
            }
            Node::Text(n) => {
                if n.chunks.iter().any(|c| matches!(c, Chunk::Ref(e) if ident_texts(e).iter().any(|t| t == wanted))) {
                    return Some(n.line);
                }
            }
            Node::ElseMarker(_) => {}
        }
    }
    None
}

fn attr_has_ref(attr: &Attr, wanted: &str) -> bool {
    match &attr.value {
        Some(crate::ast::RawValue::Str(chunks)) => chunks.iter().any(|c| {
            matches!(c, Chunk::Ref(e) if ident_texts(e).iter().any(|t| t == wanted))
        }),
        Some(crate::ast::RawValue::Ref(e)) => ident_texts(e).iter().any(|t| t == wanted),
        _ => false,
    }
}

fn ident_texts(expr: &str) -> Vec<String> {
    let mut out = Vec::new();
    for e in expr.split(['{', '}']).filter(|s| !s.is_empty()) {
        let e = e.trim();
        if e.starts_with("./") || e.starts_with("../") || e.starts_with('/') {
            continue;
        }
        let e = strip_brackets(e);
        if let Some((first, _)) = e.split_once('.') {
            if let Some(first) = first.split_whitespace().next() {
                out.push(first.to_string());
            }
        } else if let Some(first) = e.split_whitespace().next() {
            out.push(first.to_string());
        }
    }
    out
}

/// Remove `[...]` index segments from an expression, so `item[2].name`
/// shares the same scope root (`item`) as `item.name`.
fn strip_brackets(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    for c in s.chars() {
        match c {
            '[' => depth += 1,
            ']' => depth = depth.saturating_sub(1),
            other if depth == 0 => out.push(other),
            _ => {}
        }
    }
    out
}

pub fn collect_refs(nodes: &[Node], out: &mut HashSet<String>) {
    for node in nodes {
        match node {
            Node::El(n) => {
                for a in &n.attrs {
                    attr_refs(a, out);
                }
                collect_refs(&n.children, out);
            }
            Node::Inv(n) => {
                for a in &n.props {
                    attr_refs(a, out);
                }
                collect_refs(&n.children, out);
            }
            Node::Cond(n) => {
                ident_root(&n.cond, out);
                collect_refs(&n.then_ch, out);
                collect_refs(&n.else_ch, out);
            }
            Node::Loop(n) => {
                ident_root(&n.expr, out);
                collect_refs(&n.children, out);
            }
            Node::Text(n) => {
                for c in &n.chunks {
                    if let Chunk::Ref(e) = c {
                        ident_root(e, out);
                    }
                }
            }
            Node::ElseMarker(_) => {}
        }
    }
}

pub fn attr_refs(attr: &Attr, out: &mut HashSet<String>) {
    let value = match &attr.value {
        Some(v) => v,
        None => return,
    };
    match value {
        crate::ast::RawValue::Str(chunks) => {
            for c in chunks {
                if let Chunk::Ref(e) = c {
                    ident_root(e, out);
                }
            }
        }
        crate::ast::RawValue::Ref(e) => ident_root(e, out),
        _ => {}
    }
}

pub fn ident_root(expr: &str, out: &mut HashSet<String>) {
    let expr = expr.trim();
    let expr = expr.strip_prefix('{').unwrap_or(expr);
    let expr = expr.strip_suffix('}').unwrap_or(expr);
    if expr.starts_with("./") || expr.starts_with("../") || expr.starts_with('/') {
        return;
    }
    let expr = strip_brackets(expr);
    if let Some((first, _)) = expr.split_once('.') {
        if let Some(first) = first.split_whitespace().next() {
            out.insert(first.to_string());
        }
    } else if let Some(first) = expr.split_whitespace().next() {
        out.insert(first.to_string());
    }
}

/// Walk a project directory and return every `.df` file as `(rel_key, source)`,
/// with `rel_key` using `/` separators relative to `root`. The scan skips hidden
/// directories and build outputs, mirroring what the CLI collects.
pub fn walk_df_files(root: &Path) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    walk_df_dir(root, root, &mut out)?;
    Ok(out)
}

fn walk_df_dir(
    root: &Path,
    dir: &Path,
    out: &mut BTreeMap<String, String>,
) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("cannot read `{}`: {e}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|e| e.to_string())?.path();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if path.is_dir() {
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            walk_df_dir(root, &path, out)?;
        } else if name.ends_with(".df") {
            let rel = path
                .strip_prefix(root)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            let source = fs::read_to_string(&path)
                .map_err(|e| format!("cannot read `{}`: {e}", path.display()))?;
            out.insert(rel, source);
        }
    }
    Ok(())
}