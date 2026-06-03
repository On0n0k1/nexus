use std::fmt::Write;

use crate::dict::{Dictionary, FieldDef};

use super::{HEADER, byte_lit, pascal, screaming};

pub fn emit(dict: &Dictionary) -> String {
    let mut s = String::new();
    s.push_str(HEADER);

    for f in &dict.fields {
        let _ = writeln!(
            s,
            "pub const TAG_{}: u32 = {};",
            screaming(&f.name),
            f.number
        );
    }
    s.push('\n');

    for f in &dict.fields {
        if f.is_enum() {
            emit_enum(&mut s, f);
        }
    }
    s
}

fn emit_enum(s: &mut String, f: &FieldDef) {
    let ty = pascal(&f.name);
    if f.single_char() {
        emit_single_char_enum(s, f, &ty);
    } else {
        emit_multi_char_enum(s, f, &ty);
    }
}

fn emit_single_char_enum(s: &mut String, f: &FieldDef, ty: &str) {
    s.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq)]\n");
    let _ = writeln!(s, "pub enum {ty} {{");
    for v in &f.values {
        let _ = writeln!(s, "    {},", pascal(&v.name));
    }
    s.push_str("    Unknown(u8),\n}\n\n");

    let _ = writeln!(s, "impl {ty} {{");
    s.push_str("    pub fn from_byte(b: u8) -> Self {\n        match b {\n");
    for v in &f.values {
        let _ = writeln!(
            s,
            "            {} => Self::{},",
            char_lit(&v.value),
            pascal(&v.name)
        );
    }
    s.push_str("            other => Self::Unknown(other),\n        }\n    }\n\n");
    s.push_str("    pub fn as_byte(self) -> u8 {\n        match self {\n");
    for v in &f.values {
        let _ = writeln!(
            s,
            "            Self::{} => {},",
            pascal(&v.name),
            char_lit(&v.value)
        );
    }
    s.push_str("            Self::Unknown(b) => b,\n        }\n    }\n}\n\n");
}

fn emit_multi_char_enum(s: &mut String, f: &FieldDef, ty: &str) {
    s.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq)]\n");
    let _ = writeln!(s, "pub enum {ty}<'buf> {{");
    for v in &f.values {
        let _ = writeln!(s, "    {},", pascal(&v.name));
    }
    s.push_str("    Unknown(&'buf nexus_fix_codec::AsciiTextStr),\n}\n\n");

    let _ = writeln!(s, "impl<'buf> {ty}<'buf> {{");
    s.push_str(
        "    pub fn from_bytes(b: &'buf nexus_fix_codec::AsciiTextStr) -> Self {\n        match b.as_bytes() {\n",
    );
    for v in &f.values {
        let _ = writeln!(
            s,
            "            {} => Self::{},",
            byte_lit(&v.value),
            pascal(&v.name)
        );
    }
    s.push_str("            _ => Self::Unknown(b),\n        }\n    }\n\n");
    s.push_str("    pub fn as_bytes(self) -> &'buf [u8] {\n        match self {\n");
    for v in &f.values {
        let _ = writeln!(
            s,
            "            Self::{} => {},",
            pascal(&v.name),
            byte_lit(&v.value)
        );
    }
    s.push_str("            Self::Unknown(b) => b.as_bytes(),\n        }\n    }\n}\n\n");
}

fn char_lit(s: &str) -> String {
    let b = s.bytes().next().unwrap_or(b'?');
    match b {
        b'\'' | b'\\' => format!("b'\\{}'", b as char),
        0x20..=0x7e => format!("b'{}'", b as char),
        _ => format!("b'\\x{b:02x}'"),
    }
}
