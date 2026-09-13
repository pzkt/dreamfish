use crate::ast::{Attr, Chunk, CondNode, ElNode, ElseStub, InvNode, LoopNode, Node, RawValue, Target, TextNode};

#[derive(Debug, Clone)]
pub enum Tok {
    Indent,
    Dedent,
    Section {
        name: String,
        alias: Option<Target>,
        line: usize,
    },
    Chain { nodes: Vec<Node>, line: usize },
    Raw { text: String, line: usize },
}

#[derive(Debug)]
pub struct LexError {
    pub line: usize,
    pub msg: String,
}

pub fn lex(src: &str) -> Result<Vec<Tok>, LexError> {
    let mut toks = Vec::new();
    let mut indents: Vec<usize> = vec![0];
    let mut base: usize = 0;
    let mut need_base = true;
    let mut raw_mode = false;
    let mut raw_buf = String::new();

    for (idx, raw) in src.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw.trim_end_matches('\r');

        if line.starts_with('#') && leading_ws(line) == 0 {
            if let Some(decl) = parse_section_decl(line, line_no)? {
                if raw_mode {
                    toks.push(Tok::Raw {
                        text: std::mem::take(&mut raw_buf),
                        line: line_no,
                    });
                    raw_mode = false;
                }
                flush_indents(&mut toks, &mut indents, 0);
                indents = vec![0];
                base = 0;
                need_base = true;
                toks.push(Tok::Section {
                    name: decl.name.clone(),
                    alias: decl.alias,
                    line: line_no,
                });
                if is_asset_name(&decl.name) {
                    raw_mode = true;
                    raw_buf.clear();
                }
                continue;
            }
        }

        if raw_mode {
            raw_buf.push_str(line);
            raw_buf.push('\n');
            continue;
        }

        if is_blank_or_comment(line) {
            continue;
        }

        let w = leading_ws(line);
        if need_base {
            base = w;
            need_base = false;
        }
        let rel = w.saturating_sub(base);

        let top = *indents.last().unwrap();
        if rel > top {
            indents.push(rel);
            toks.push(Tok::Indent);
        } else if rel < top {
            if !indents.contains(&rel) {
                return Err(LexError {
                    line: line_no,
                    msg: format!("inconsistent indentation (found depth {rel})"),
                });
            }
            while *indents.last().unwrap() > rel {
                indents.pop();
                toks.push(Tok::Dedent);
            }
        }

        let nodes = parse_content_line(line.trim_start(), line_no)?;
        toks.push(Tok::Chain {
            nodes,
            line: line_no,
        });
    }

    if raw_mode {
        toks.push(Tok::Raw {
            text: std::mem::take(&mut raw_buf),
            line: 0,
        });
    }

    Ok(toks)
}

fn flush_indents(toks: &mut Vec<Tok>, indents: &mut Vec<usize>, to: usize) {
    while indents.len() > to + 1 {
        indents.pop();
        toks.push(Tok::Dedent);
    }
}

fn is_blank_or_comment(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.is_empty() || trimmed.starts_with('%')
}

fn leading_ws(line: &str) -> usize {
    line.chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .map(|c| if c == '\t' { 4 } else { 1 })
        .sum()
}

struct SectionDecl {
    name: String,
    alias: Option<Target>,
}

fn parse_section_decl(line: &str, line_no: usize) -> Result<Option<SectionDecl>, LexError> {
    let rest = &line[1..];
    let name = section_name(rest);
    if name.is_empty() {
        return Ok(None);
    }

    let after_name = rest[name.len()..].trim_start();

    if after_name.is_empty() {
        return Ok(Some(SectionDecl {
            name,
            alias: None,
        }));
    }

    if let Some(target_expr) = after_name.strip_prefix('>') {
        let target = parse_target(target_expr.trim(), line_no)?;
        return Ok(Some(SectionDecl {
            name,
            alias: Some(target),
        }));
    }

    Ok(None)
}

fn section_name(rest: &str) -> String {
    let mut name = String::new();
    let mut chars = rest.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_alphanumeric() || c == '_' {
            name.push(c);
            chars.next();
        } else {
            break;
        }
    }
    if chars.peek() == Some(&'.') {
        let mut probe = name.clone();
        probe.push('.');
        chars.next();
        if let Some(&c) = chars.peek() {
            if c.is_ascii_alphabetic() {
                probe.push(c);
                chars.next();
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_alphanumeric() || c == '_' {
                        probe.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                return probe;
            }
        }
    }
    name
}

fn is_asset_name(name: &str) -> bool {
    name.ends_with(".css") || name.ends_with(".js")
}

fn parse_target(s: &str, line_no: usize) -> Result<Target, LexError> {
    let s = s.trim();
    if s.starts_with('{') && s.ends_with('}') {
        return Ok(Target::Var(s[1..s.len() - 1].trim().to_string()));
    }
    if s.starts_with("./") || s.starts_with("../") || s.starts_with('/') {
        if let Some((path, section)) = s.split_once('#') {
            let section = section.trim();
            let section = if section.is_empty() {
                None
            } else {
                Some(section.to_string())
            };
            Ok(Target::File {
                path: path.trim().to_string(),
                section,
            })
        } else {
            Ok(Target::File {
                path: s.to_string(),
                section: None,
            })
        }
    } else if s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        Ok(Target::Local(s.to_string()))
    } else {
        Err(LexError {
            line: line_no,
            msg: format!("invalid target expression `{s}`"),
        })
    }
}

fn parse_content_line(line: &str, line_no: usize) -> Result<Vec<Node>, LexError> {
    let segments = split_chain(line)?;
    let mut nodes = Vec::new();
    for seg in segments {
        let seg = seg.trim();
        if seg.is_empty() {
            continue;
        }
        nodes.push(parse_segment(seg, line_no)?);
    }
    if nodes.is_empty() {
        return Err(LexError {
            line: line_no,
            msg: "empty content line".into(),
        });
    }
    Ok(nodes)
}

fn split_chain(line: &str) -> Result<Vec<String>, LexError> {
    let mut segments = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut braces = 0usize;
    for c in line.chars() {
        match quote {
            Some(q) => {
                cur.push(c);
                if c == q {
                    quote = None;
                }
            }
            None => match c {
                '"' | '\'' => {
                    quote = Some(c);
                    cur.push(c);
                }
                '{' => {
                    braces += 1;
                    cur.push(c);
                }
                '}' => {
                    braces = braces.saturating_sub(1);
                    cur.push(c);
                }
                '>' if braces == 0 => {
                    segments.push(cur.trim().to_string());
                    cur.clear();
                }
                _ => cur.push(c),
            },
        }
    }
    segments.push(cur.trim().to_string());
    Ok(segments)
}

fn parse_segment(seg: &str, line_no: usize) -> Result<Node, LexError> {
    if let Some(rest) = seg.strip_prefix('<') {
        if let Some(left) = rest.strip_prefix(':') {
            return parse_invoke(left, line_no);
        }
        return parse_element(rest, line_no);
    }
    let chunks = parse_chunks(seg);
    Ok(Node::Text(TextNode { line: line_no, chunks }))
}

fn parse_element(rest: &str, line_no: usize) -> Result<Node, LexError> {
    let tokens = tokenize_ws(rest)?;
    if tokens.is_empty() {
        return Err(LexError {
            line: line_no,
            msg: "expected a tagname after `<`".into(),
        });
    }
    let tag = tokens[0].clone();
    let attrs = parse_attrs(&tokens[1..], line_no)?;
    Ok(Node::El(ElNode {
        line: line_no,
        tag,
        attrs,
        children: Vec::new(),
    }))
}

fn parse_invoke(rest: &str, line_no: usize) -> Result<Node, LexError> {
    let tokens = tokenize_ws(rest)?;
    if tokens.is_empty() {
        return Err(LexError {
            line: line_no,
            msg: "expected a target after `<:`".into(),
        });
    }
    let first = &tokens[0];
    if first == "if" {
        let cond = tokens[1..].join(" ");
        return Ok(Node::Cond(CondNode {
            line: line_no,
            cond: cond.trim().to_string(),
            then_ch: Vec::new(),
            else_ch: Vec::new(),
        }));
    }
    if first == "else" {
        if tokens.len() != 1 {
            return Err(LexError {
                line: line_no,
                msg: "unexpected tokens after `<:else`".into(),
            });
        }
        return Ok(Node::ElseMarker(ElseStub {
            line: line_no,
            children: Vec::new(),
        }));
    }
    if first == "for" {
        if tokens.len() < 4 || tokens[2] != "in" {
            return Err(LexError {
                line: line_no,
                msg: "expected `<:for var in expr`".into(),
            });
        }
        let var = tokens[1].clone();
        let expr = tokens[3..].join(" ");
        return Ok(Node::Loop(LoopNode {
            line: line_no,
            var,
            expr: expr.trim().to_string(),
            children: Vec::new(),
        }));
    }
    let target = parse_target(first, line_no)?;
    let props = parse_attrs(&tokens[1..], line_no)?;
    Ok(Node::Inv(InvNode {
        line: line_no,
        target,
        props,
        children: Vec::new(),
    }))
}

pub fn tokenize_ws(input: &str) -> Result<Vec<String>, LexError> {
    let mut toks = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut braces = 0usize;
    for c in input.chars() {
        match quote {
            Some(q) => {
                cur.push(c);
                if c == q {
                    quote = None;
                }
            }
            None => match c {
                '"' | '\'' => {
                    quote = Some(c);
                    cur.push(c);
                }
                '{' => {
                    braces += 1;
                    cur.push(c);
                }
                '}' => {
                    braces = braces.saturating_sub(1);
                    cur.push(c);
                }
                c if c.is_whitespace() && braces == 0 => {
                    if !cur.is_empty() {
                        toks.push(std::mem::take(&mut cur));
                    }
                }
                _ => cur.push(c),
            },
        }
    }
    if !cur.is_empty() {
        toks.push(cur);
    }
    Ok(toks)
}

fn parse_attrs(tokens: &[String], line_no: usize) -> Result<Vec<Attr>, LexError> {
    let mut attrs = Vec::new();
    for tok in tokens {
        if let Some((k, v)) = split_key_eq(tok) {
            let value = parse_value(v, line_no)?;
            attrs.push(Attr {
                key: k.to_string(),
                value: Some(value),
            });
        } else {
            attrs.push(Attr {
                key: tok.clone(),
                value: None,
            });
        }
    }
    Ok(attrs)
}

fn split_key_eq(tok: &str) -> Option<(&str, &str)> {
    let mut quote: Option<char> = None;
    for (i, c) in tok.char_indices() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
            }
            None => {
                if c == '"' || c == '\'' {
                    quote = Some(c);
                } else if c == '=' {
                    return Some((&tok[..i], &tok[i + 1..]));
                }
            }
        }
    }
    None
}

fn parse_value(v: &str, _line_no: usize) -> Result<RawValue, LexError> {
    let v = v.trim();
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        let inner = &v[1..v.len() - 1];
        return Ok(RawValue::Str(parse_chunks(inner)));
    }
    if v == "True" || v == "true" {
        return Ok(RawValue::Bool(true));
    }
    if v == "False" || v == "false" {
        return Ok(RawValue::Bool(false));
    }
    if v == "Null" || v == "null" {
        return Ok(RawValue::Null);
    }
    if v.len() >= 2 && v.starts_with('{') && v.ends_with('}') {
        return Ok(RawValue::Ref(v[1..v.len() - 1].trim().to_string()));
    }
    if let Ok(n) = v.parse::<f64>() {
        return Ok(RawValue::Num(n));
    }
    Ok(RawValue::Str(parse_chunks(v)))
}

pub fn parse_chunks(s: &str) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut lit = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '{' {
            let mut expr = String::new();
            let mut depth = 1usize;
            for c2 in chars.by_ref() {
                if c2 == '{' {
                    depth += 1;
                } else if c2 == '}' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                expr.push(c2);
            }
            if !lit.is_empty() {
                chunks.push(Chunk::Lit(std::mem::take(&mut lit)));
            }
            chunks.push(Chunk::Ref(expr.trim().to_string()));
        } else {
            lit.push(c);
        }
    }
    if !lit.is_empty() {
        chunks.push(Chunk::Lit(lit));
    }
    chunks
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dump_tokens() {
        let src = "\
<:shortBtn label=\"Go\"
<:BigBlock title=\"Hello {name}\" show=truthy
<:for b in {./flags.kdl}
 <div class=\"flag\"
  <:if {b.bigThing}
   <span > BIG
  <:else
   <span > small
";
        let toks = lex(src).unwrap();
        for (i, t) in toks.iter().enumerate() {
            println!("[{i}] {:?}", t);
        }
    }
}
