use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Str(String),
    Bool(bool),
    Null,
    Num(f64),
    Lst(Vec<Value>),
    Rec(BTreeMap<String, Value>),
}

impl Value {
    pub fn truthy(&self) -> bool {
        match self {
            Value::Str(s) => !s.is_empty(),
            Value::Bool(b) => *b,
            Value::Null => false,
            Value::Num(n) => *n != 0.0,
            Value::Lst(l) => !l.is_empty(),
            Value::Rec(r) => !r.is_empty(),
        }
    }

    pub fn get(&self, field: &str) -> Option<Value> {
        match self {
            Value::Rec(r) => r.get(field).cloned(),
            _ => None,
        }
    }

    pub fn index_num(&self, n: i64) -> Option<Value> {
        let list = match self {
            Value::Rec(r) => {
                if let Some(Value::Lst(l)) = r.get("_args") {
                    l
                } else {
                    return None;
                }
            }
            Value::Lst(l) => l,
            _ => return None,
        };
        let idx = if n < 0 {
            let abs = n.unsigned_abs() as usize;
            if abs > list.len() {
                return None;
            }
            list.len() - abs
        } else {
            n as usize
        };
        list.get(idx).cloned()
    }

    pub fn collapse_args(self) -> Self {
        match self {
            Value::Rec(mut r) => {
                if let Some(Value::Lst(args)) = r.remove("_args") {
                    match args.len() {
                        1 => args.into_iter().next().unwrap(),
                        n if n > 1 => Value::Lst(args),
                        _ => Value::Rec(r),
                    }
                } else {
                    Value::Rec(r)
                }
            }
            other => other,
        }
    }

    pub fn to_text(&self) -> String {
        match self {
            Value::Str(s) => s.clone(),
            Value::Bool(b) => {
                if *b {
                    "True".into()
                } else {
                    "False".into()
                }
            }
            Value::Null => String::new(),
            Value::Num(n) => format_number(*n),
            Value::Lst(items) => {
                let inner: Vec<String> = items.iter().map(|v| v.to_text()).collect();
                format!("[{}]", inner.join(", "))
            }
            Value::Rec(map) => {
                let inner: Vec<String> = map
                    .iter()
                    .map(|(k, v)| format!("\"{k}\": {}", v.to_text()))
                    .collect();
                format!("{{{}}}", inner.join(", "))
            }
        }
    }
}

pub fn format_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 {
        format!("{}", n as i64)
    } else {
        n.to_string()
    }
}

#[derive(Debug, Clone, PartialEq)]
enum KTok {
    Ident(String),
    Str(String),
    Num(f64),
    LBrace,
    RBrace,
    Eq,
}

pub fn parse_kdl(src: &str) -> Result<Value, String> {
    let toks = klex(src)?;
    let (nodes, pos) = parse_node_list(&toks, 0)?;
    if pos != toks.len() {
        return Err(format!("unexpected trailing KDL tokens at position {pos}"));
    }
    if nodes.is_empty() {
        return Ok(Value::Lst(Vec::new()));
    }
    if nodes.len() == 1 {
        let node = nodes.into_iter().next().unwrap();
        if let Some(ref name) = node.name {
            if !name.is_empty()
                && name != "_"
                && node.props.is_empty()
                && node.children.is_empty()
                && node.args.len() == 1
            {
                return Ok(Value::Rec(BTreeMap::from([(
                    name.clone(),
                    node.args.into_iter().next().unwrap(),
                )])));
            }
        }
        return node_to_value(node);
    }
    let mut rec = BTreeMap::new();
    let mut contents = Vec::new();
    for node in nodes {
        let key = node.name.clone().unwrap_or_else(|| "_".to_string());
        let content = node_to_value(node)?;
        rec.insert(key, content.clone());
        contents.push(content);
    }
    rec.insert("_args".into(), Value::Lst(contents));
    Ok(Value::Rec(rec))
}

struct KNode {
    name: Option<String>,
    args: Vec<Value>,
    props: Vec<(String, Value)>,
    children: Vec<KNode>,
}

fn node_to_value(n: KNode) -> Result<Value, String> {
    let mut rec = BTreeMap::new();
    for (k, v) in n.props {
        rec.insert(k, v);
    }
    rec.insert("_args".into(), Value::Lst(n.args));
    if !n.children.is_empty() {
        let mut groups: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        for child in n.children {
            let label = child
                .name
                .clone()
                .unwrap_or_else(|| "_args".to_string());
            groups
                .entry(label)
                .or_default()
                .push(node_to_value(child)?);
        }
        for (k, mut lst) in groups {
            if lst.len() == 1 {
                rec.insert(k, lst.remove(0));
            } else {
                rec.insert(k, Value::Lst(lst));
            }
        }
    }
    Ok(Value::Rec(rec))
}

fn klex(src: &str) -> Result<Vec<(KTok, usize)>, String> {
    let mut toks = Vec::new();
    let mut chars = src.chars().peekable();
    let mut line: usize = 1;
    while let Some(&c) = chars.peek() {
        match c {
            c if c.is_whitespace() => {
                if c == '\n' {
                    line += 1;
                }
                chars.next();
            }
            '/' if chars.clone().nth(1) == Some('/') => {
                chars.next();
                chars.next();
                while let Some(&c) = chars.peek() {
                    if c == '\n' {
                        break;
                    }
                    chars.next();
                }
            }
            '/' if chars.clone().nth(1) == Some('*') => {
                chars.next();
                chars.next();
                let mut depth = 1usize;
                while let Some(&c) = chars.peek() {
                    if c == '/' && chars.clone().nth(1) == Some('*') {
                        depth += 1;
                        chars.next();
                        chars.next();
                    } else if c == '*' && chars.clone().nth(1) == Some('/') {
                        depth -= 1;
                        chars.next();
                        chars.next();
                        if depth == 0 {
                            break;
                        }
                    } else {
                        if c == '\n' {
                            line += 1;
                        }
                        chars.next();
                    }
                }
            }
            '"' => {
                chars.next();
                let mut s = String::new();
                while let Some(&c) = chars.peek() {
                    if c == '"' {
                        chars.next();
                        break;
                    }
                    if c == '\\' {
                        chars.next();
                        if let Some(&ec) = chars.peek() {
                            match ec {
                                'n' => s.push('\n'),
                                't' => s.push('\t'),
                                'r' => s.push('\r'),
                                other => s.push(other),
                            }
                            chars.next();
                            continue;
                        }
                    }
                    s.push(c);
                    chars.next();
                }
                toks.push((KTok::Str(s), line));
            }
            '{' | '}' | '=' => {
                let t = match c {
                    '{' => KTok::LBrace,
                    '}' => KTok::RBrace,
                    _ => KTok::Eq,
                };
                toks.push((t, line));
                chars.next();
            }
            c if c == '-' || c == '+' || c.is_ascii_digit() => {
                let mut num = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_digit() || c == '.' || c == '-' || c == '+' || c == 'e' || c == 'E' {
                        num.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                match num.parse::<f64>() {
                    Ok(n) => toks.push((KTok::Num(n), line)),
                    Err(_) => return Err(format!("invalid number literal `{num}`")),
                }
            }
            c if c.is_ascii_alphanumeric()
                || c == '_'
                || c == '-'
                || c == '.'
                || c == '#' =>
            {
                let mut ident = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' || c == '#' {
                        ident.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                toks.push((KTok::Ident(ident), line));
            }
            other => {
                return Err(format!("unexpected character `{other}` in KDL"));
            }
        }
    }
    Ok(toks)
}

fn literal(tok: &KTok) -> Result<Value, String> {
    Ok(match tok {
        KTok::Str(s) => Value::Str(s.clone()),
        KTok::Num(n) => Value::Num(*n),
        KTok::Ident(s) => match s.as_str() {
            "#true" | "true" => Value::Bool(true),
            "#false" | "false" => Value::Bool(false),
            "#null" | "null" => Value::Null,
            other => Value::Str(other.to_string()),
        },
        _ => return Err("expected a literal value".into()),
    })
}

fn parse_node_list(toks: &[(KTok, usize)], pos: usize) -> Result<(Vec<KNode>, usize), String> {
    let mut nodes = Vec::new();
    let mut p = pos;
    while p < toks.len() {
        if toks[p].0 == KTok::RBrace {
            return Ok((nodes, p));
        }
        let (node, next) = parse_single_node(toks, p)?;
        nodes.push(node);
        p = next;
    }
    Ok((nodes, p))
}

fn parse_single_node(toks: &[(KTok, usize)], pos: usize) -> Result<(KNode, usize), String> {
    let mut p = pos;
    let mut name: Option<String> = None;

    if p < toks.len() {
        if let KTok::Ident(s) = &toks[p].0 {
            let next_is_eq = toks.get(p + 1).map(|t| t.0 == KTok::Eq).unwrap_or(false);
            if !next_is_eq {
                name = Some(s.clone());
                p += 1;
            }
        }
    }

    let mut args = Vec::new();
    let mut props = Vec::new();
    let mut last_line: Option<usize> = None;

    loop {
        if p >= toks.len() {
            break;
        }
        let (tok, cur_line) = &toks[p];
        if let Some(ll) = last_line {
            if *cur_line != ll {
                break;
            }
        }
        match tok {
            KTok::LBrace => {
                p += 1;
                let (children, np) = parse_node_list(toks, p)?;
                if np >= toks.len() || toks[np].0 != KTok::RBrace {
                    return Err("unclosed `{{` in KDL node".into());
                }
                let node = KNode {
                    name,
                    args,
                    props,
                    children,
                };
                return Ok((node, np + 1));
            }
            KTok::Ident(k) if toks.get(p + 1).map(|t| t.0 == KTok::Eq).unwrap_or(false) => {
                if let Some(vt) = toks.get(p + 2) {
                    let v = literal(&vt.0)?;
                    props.push((k.clone(), v));
                    p += 3;
                    last_line = Some(*cur_line);
                    continue;
                }
                break;
            }
            _ => {
                let v = literal(tok)?;
                args.push(v);
                p += 1;
                last_line = Some(*cur_line);
            }
        }
    }

    Ok((
        KNode {
            name,
            args,
            props,
            children: Vec::new(),
        },
        p,
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dump_flags() {
        let src = "bigThing true\notherThing false\n";
        let v = parse_kdl(src).unwrap();
        println!("flags -> {:?}", v);
        let rows = "row name=\"peter\" score=9\nrow name=\"anna\" score=7.5\n";
        println!("rows -> {:?}", parse_kdl(rows).unwrap());
    }

    #[test]
    fn multiple_top_level_nodes() {
        let src = "server name=\"prod\" {\n    host \"example.com\"\n    port 443\n}\n\
                   server name=\"staging\" {\n    host \"staging.example.com\"\n    port 80\n}\n";
        match parse_kdl(src).unwrap() {
            Value::Rec(rec) => {
                // ordered contents are preserved for `[n]` and iteration
                match rec.get("_args") {
                    Some(Value::Lst(items)) => {
                        assert_eq!(items.len(), 2, "one entry per top-level node");
                        for item in items {
                            assert!(matches!(item, Value::Rec(_)));
                        }
                    }
                    other => panic!("expected `_args` with node contents, got {other:?}"),
                }
                // node names become addressable keys (last one wins on repeats)
                let server = rec.get("server").unwrap().clone().collapse_args();
                match server {
                    Value::Rec(s) => {
                        assert_eq!(
                            s.get("host").cloned().unwrap().collapse_args(),
                            Value::Str("staging.example.com".into())
                        );
                        assert_eq!(
                            s.get("name").cloned().unwrap().collapse_args(),
                            Value::Str("staging".into())
                        );
                    }
                    other => panic!("expected server record, got {other:?}"),
                }
            }
            other => panic!("expected a record keyed by node name, got {other:?}"),
        }
        // distinct names: each is reachable by its node name
        let v = parse_kdl("alpha \"x\"\nbeta \"y\"\n").unwrap();
        match &v {
            Value::Rec(rec) => {
                assert_eq!(
                    rec.get("alpha").cloned().unwrap().collapse_args(),
                    Value::Str("x".into())
                );
                assert_eq!(
                    rec.get("beta").cloned().unwrap().collapse_args(),
                    Value::Str("y".into())
                );
            }
            other => panic!("expected a named record, got {other:?}"),
        }
    }
}
