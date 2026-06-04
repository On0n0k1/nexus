use std::collections::HashSet;
use std::fmt::Write;

use super::{
    HEADER, RGroup, RMember, RMessage, emit_group_accessor, emit_value_accessor, group_type,
    pascal, screaming, snake, subtree_tags, tag_or,
};

pub fn emit(messages: &[RMessage]) -> String {
    let mut s = String::new();
    s.push_str(HEADER);
    for m in messages {
        let prefix = pascal(&m.name);
        for mem in &m.members {
            if let RMember::Group(g) = mem {
                emit_group(&mut s, &prefix, g);
            }
        }
    }
    s
}

fn emit_group(s: &mut String, prefix: &str, g: &RGroup) {
    let base = group_type(prefix, &g.name);
    let entry = format!("{base}Entry");
    let iter = format!("{base}Iter");

    let _ = write!(
        s,
        "pub struct {iter}<'buf> {{\n    buf: &'buf [u8],\n    pos: usize,\n    remaining: u16,\n}}\n\n"
    );
    let _ = writeln!(s, "impl<'buf> {iter}<'buf> {{");
    let _ = write!(
        s,
        "    pub fn new(buf: &'buf [u8], span: nexus_fix_codec::GroupSpan) -> Self {{\n        Self {{ buf, pos: span.offset as usize, remaining: span.count }}\n    }}\n}}\n\n"
    );
    let _ = writeln!(
        s,
        "impl<'buf> Iterator for {iter}<'buf> {{\n    type Item = {entry}<'buf>;"
    );
    let _ = write!(
        s,
        "    fn next(&mut self) -> Option<Self::Item> {{\n        if self.remaining == 0 {{\n            return None;\n        }}\n        self.remaining -= 1;\n        let (e, next) = {entry}::decode(self.buf, self.pos);\n        self.pos = next;\n        Some(e)\n    }}\n}}\n\n"
    );

    emit_entry(s, &base, &entry, g);

    for mem in &g.members {
        if let RMember::Group(inner) = mem {
            emit_group(s, &base, inner);
        }
    }
}

fn emit_entry(s: &mut String, base: &str, entry: &str, g: &RGroup) {
    let _ = writeln!(s, "pub struct {entry}<'buf> {{\n    buf: &'buf [u8],");
    let mut seen = HashSet::new();
    for mem in &g.members {
        match mem {
            RMember::Field(f) if seen.insert(f.number) => {
                let _ = writeln!(s, "    {}: nexus_fix_codec::FieldSpan,", snake(&f.name));
            }
            RMember::Group(inner) if seen.insert(inner.number) => {
                let _ = writeln!(s, "    {}: nexus_fix_codec::GroupSpan,", snake(&inner.name));
            }
            _ => {}
        }
    }
    s.push_str("}\n\n");

    let mut tags = Vec::new();
    subtree_tags(&g.members, &mut tags);
    let pat = tag_or(&tags);

    let _ = writeln!(s, "impl<'buf> {entry}<'buf> {{");
    s.push_str(
        "    fn decode(buf: &'buf [u8], start: usize) -> (Self, usize) {\n        let mut e = Self {\n            buf,\n",
    );
    let mut seen = HashSet::new();
    for mem in &g.members {
        match mem {
            RMember::Field(f) if seen.insert(f.number) => {
                let _ = writeln!(
                    s,
                    "            {}: nexus_fix_codec::FieldSpan::EMPTY,",
                    snake(&f.name)
                );
            }
            RMember::Group(inner) if seen.insert(inner.number) => {
                let _ = writeln!(
                    s,
                    "            {}: nexus_fix_codec::GroupSpan::EMPTY,",
                    snake(&inner.name)
                );
            }
            _ => {}
        }
    }
    s.push_str("        };\n");
    s.push_str("        let mut r = nexus_fix_codec::FieldReader::new(buf, start);\n");
    s.push_str("        let mut first = true;\n");
    s.push_str("        loop {\n            let mark = r.pos();\n");
    s.push_str("            let Some(f) = r.next_field() else { break };\n");
    let _ = writeln!(
        s,
        "            if (f.tag == {} && !first) || !matches!(f.tag, {pat}) {{\n                return (e, mark);\n            }}",
        g.delimiter
    );
    s.push_str("            first = false;\n");

    let mut arms: Vec<(String, String)> = Vec::new();
    let mut seen_arm = HashSet::new();
    for mem in &g.members {
        match mem {
            RMember::Field(f) if seen_arm.insert(f.number) => {
                arms.push((
                    screaming(&f.name),
                    format!("                e.{} = f.value;\n", snake(&f.name)),
                ));
            }
            RMember::Group(inner) if seen_arm.insert(inner.number) => {
                arms.push((screaming(&inner.name), nested_body(inner)));
            }
            _ => {}
        }
    }
    emit_entry_dispatch(s, &arms);
    s.push_str("        }\n        (e, r.pos())\n    }\n\n");

    emit_entry_accessors(s, base, g);
    s.push_str("}\n\n");
}

fn emit_entry_dispatch(s: &mut String, arms: &[(String, String)]) {
    if let [(tag, body)] = arms {
        let _ = writeln!(s, "            if f.tag == super::fields::TAG_{tag} {{");
        s.push_str(body);
        s.push_str("            }\n");
    } else {
        s.push_str("            match f.tag {\n");
        for (tag, body) in arms {
            let _ = writeln!(s, "                super::fields::TAG_{tag} => {{");
            s.push_str(body);
            s.push_str("                }\n");
        }
        s.push_str("                _ => {}\n            }\n");
    }
}

fn nested_body(inner: &RGroup) -> String {
    let mut tags = Vec::new();
    subtree_tags(&inner.members, &mut tags);
    let pat = tag_or(&tags);
    let mut b = String::new();
    b.push_str(
        "                let (count, _) = nexus_fix_codec::parse_tag(f.value.slice(buf));\n",
    );
    let _ = writeln!(
        b,
        "                e.{} = nexus_fix_codec::GroupSpan::new(r.pos() as u32, count.min(u16::MAX as u32) as u16);",
        snake(&inner.name)
    );
    b.push_str("                loop {\n                    let nmark = r.pos();\n");
    b.push_str("                    match r.next_field() {\n");
    let _ = writeln!(
        b,
        "                        Some(nf) if matches!(nf.tag, {pat}) => {{}}"
    );
    b.push_str("                        _ => {\n                            r = nexus_fix_codec::FieldReader::new(buf, nmark);\n                            break;\n                        }\n");
    b.push_str("                    }\n                }\n");
    b
}

fn emit_entry_accessors(s: &mut String, base: &str, g: &RGroup) {
    let mut seen = HashSet::new();
    for mem in &g.members {
        match mem {
            RMember::Field(f) if seen.insert(f.number) => emit_value_accessor(s, f, "self.buf"),
            RMember::Group(inner) if seen.insert(inner.number) => {
                let iter = format!("{}Iter", group_type(base, &inner.name));
                emit_group_accessor(s, &snake(&inner.name), &iter, "self.buf");
            }
            _ => {}
        }
    }
}
