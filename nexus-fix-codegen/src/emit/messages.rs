use std::collections::{HashMap, HashSet};
use std::fmt::Write;

use crate::dict::FieldType;

use super::{
    HEADER, RField, RGroup, RMember, RMessage, emit_group_accessor, emit_value_accessor,
    group_type, pascal, screaming, snake, subtree_tags, tag_or,
};

enum Top<'a> {
    Field(&'a RField),
    Group(&'a RGroup),
}

pub fn emit(messages: &[RMessage]) -> String {
    let mut s = String::new();
    s.push_str(HEADER);
    for m in messages {
        emit_message(&mut s, m);
    }
    s
}

fn emit_message(s: &mut String, m: &RMessage) {
    let ty = pascal(&m.name);
    let tops: Vec<Top> = m
        .members
        .iter()
        .map(|mem| match mem {
            RMember::Field(f) => Top::Field(f),
            RMember::Group(g) => Top::Group(g),
        })
        .collect();

    let mut data_handled: HashSet<u32> = HashSet::new();
    let mut data_after: HashMap<u32, &RField> = HashMap::new();
    for w in tops.windows(2) {
        if let [Top::Field(l), Top::Field(d)] = w
            && l.ftype == FieldType::Length
            && d.ftype == FieldType::Data
        {
            data_handled.insert(d.number);
            data_after.insert(l.number, *d);
        }
    }

    emit_struct(s, &ty, &tops);
    let _ = writeln!(s, "impl<'buf> {ty}<'buf> {{");
    emit_decode(s, &tops, &data_handled, &data_after);
    emit_is_complete(s, &tops);
    emit_accessors(s, &tops, &m.name);
    s.push_str("}\n\n");
}

fn emit_struct(s: &mut String, ty: &str, tops: &[Top]) {
    let _ = writeln!(s, "pub struct {ty}<'buf> {{\n    buf: &'buf [u8],");
    let mut seen = HashSet::new();
    for t in tops {
        match t {
            Top::Field(f) if seen.insert(f.number) => {
                let _ = writeln!(s, "    {}: nexus_fix_codec::FieldSpan,", snake(&f.name));
            }
            Top::Group(g) if seen.insert(g.number) => {
                let _ = writeln!(s, "    {}: nexus_fix_codec::GroupSpan,", snake(&g.name));
            }
            _ => {}
        }
    }
    s.push_str("}\n\n");
}

fn emit_decode(
    s: &mut String,
    tops: &[Top],
    data_handled: &HashSet<u32>,
    data_after: &HashMap<u32, &RField>,
) {
    s.push_str("    pub fn decode(buf: &'buf [u8]) -> Self {\n");
    s.push_str("        let mut m = Self {\n            buf,\n");
    let mut seen = HashSet::new();
    for t in tops {
        match t {
            Top::Field(f) if seen.insert(f.number) => {
                let _ = writeln!(
                    s,
                    "            {}: nexus_fix_codec::FieldSpan::EMPTY,",
                    snake(&f.name)
                );
            }
            Top::Group(g) if seen.insert(g.number) => {
                let _ = writeln!(
                    s,
                    "            {}: nexus_fix_codec::GroupSpan::EMPTY,",
                    snake(&g.name)
                );
            }
            _ => {}
        }
    }
    s.push_str("        };\n");

    let mut arms: Vec<(String, String)> = Vec::new();
    let mut seen_arm = HashSet::new();
    for t in tops {
        match t {
            Top::Field(f) => {
                if data_handled.contains(&f.number) || !seen_arm.insert(f.number) {
                    continue;
                }
                if let Some(d) = data_after.get(&f.number) {
                    arms.push((screaming(&f.name), data_body(f, d)));
                } else {
                    arms.push((
                        screaming(&f.name),
                        format!("                m.{} = f.value;\n", snake(&f.name)),
                    ));
                }
            }
            Top::Group(g) => {
                if !seen_arm.insert(g.number) {
                    continue;
                }
                arms.push((screaming(&g.name), group_body(g)));
            }
        }
    }

    emit_dispatch(s, &arms);
    s.push_str("        m\n    }\n\n");
}

fn emit_dispatch(s: &mut String, arms: &[(String, String)]) {
    if arms.is_empty() {
        return;
    }
    s.push_str("        let mut r = nexus_fix_codec::FieldReader::new(buf, 0);\n");
    s.push_str("        while let Some(f) = r.next_field() {\n");
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
    s.push_str("        }\n");
}

fn data_body(len: &RField, data: &RField) -> String {
    let mut b = String::new();
    let _ = writeln!(b, "                m.{} = f.value;", snake(&len.name));
    b.push_str("                let (n, _) = nexus_fix_codec::parse_tag(f.value.slice(buf));\n");
    b.push_str("                let dstart = r.pos();\n");
    b.push_str("                let (_, dtl) = nexus_fix_codec::parse_tag(&buf[dstart..]);\n");
    b.push_str("                let vstart = dstart + dtl + 1;\n");
    b.push_str("                let dlen = (n as usize).min(buf.len().saturating_sub(vstart));\n");
    let _ = writeln!(
        b,
        "                m.{} = nexus_fix_codec::FieldSpan::new(vstart as u32, dlen as u32);",
        snake(&data.name)
    );
    b.push_str(
        "                r = nexus_fix_codec::FieldReader::new(buf, (vstart + dlen + 1).min(buf.len()));\n",
    );
    b
}

fn group_body(g: &RGroup) -> String {
    let mut tags = Vec::new();
    subtree_tags(&g.members, &mut tags);
    let pat = tag_or(&tags);
    let mut b = String::new();
    b.push_str(
        "                let (count, _) = nexus_fix_codec::parse_tag(f.value.slice(buf));\n",
    );
    let _ = writeln!(
        b,
        "                m.{} = nexus_fix_codec::GroupSpan::new(r.pos() as u32, count.min(u16::MAX as u32) as u16);",
        snake(&g.name)
    );
    b.push_str("                loop {\n");
    b.push_str("                    let mark = r.pos();\n");
    b.push_str("                    match r.next_field() {\n");
    let _ = writeln!(
        b,
        "                        Some(gf) if matches!(gf.tag, {pat}) => {{}}"
    );
    b.push_str("                        _ => {\n");
    b.push_str("                            r = nexus_fix_codec::FieldReader::new(buf, mark);\n");
    b.push_str("                            break;\n");
    b.push_str("                        }\n");
    b.push_str("                    }\n                }\n");
    b
}

fn emit_is_complete(s: &mut String, tops: &[Top]) {
    let mut conds = Vec::new();
    let mut seen = HashSet::new();
    for t in tops {
        match t {
            Top::Field(f) if f.required && seen.insert(f.number) => {
                conds.push(format!("self.{}.is_present()", snake(&f.name)));
            }
            Top::Group(g) if g.required && seen.insert(g.number) => {
                conds.push(format!("self.{}.is_present()", snake(&g.name)));
            }
            _ => {}
        }
    }
    let body = if conds.is_empty() {
        "true".to_string()
    } else {
        conds.join(" && ")
    };
    let _ = writeln!(
        s,
        "    pub fn is_complete(&self) -> bool {{\n        {body}\n    }}\n"
    );
}

fn emit_accessors(s: &mut String, tops: &[Top], msg_name: &str) {
    let prefix = pascal(msg_name);
    let mut seen = HashSet::new();
    for t in tops {
        match t {
            Top::Field(f) if seen.insert(f.number) => emit_value_accessor(s, f),
            Top::Group(g) if seen.insert(g.number) => {
                let iter = format!("{}Iter", group_type(&prefix, &g.name));
                emit_group_accessor(s, &snake(&g.name), &iter);
            }
            _ => {}
        }
    }
}
