use std::collections::HashSet;
use std::fmt::Write;

use super::{HEADER, RField, RMember, RMessage, emit_value_accessor, snake};

/// (tag, snake_field_name, rust_return_type)
type ProtocolField = (u32, &'static str, &'static str);

/// (struct_name, msgtype_byte, protocol_required_fields)
const SPECS: &[(&str, &str, &[ProtocolField])] = &[
    (
        "Logon",
        "A",
        &[
            (108, "heart_bt_int", "u32"),
            (98, "encrypt_method", "u32"),
            (141, "reset_seq_num_flag", "bool"),
        ],
    ),
    (
        "Logout",
        "5",
        &[(58, "text", "&'buf nexus_fix_codec::AsciiTextStr")],
    ),
    (
        "Heartbeat",
        "0",
        &[(112, "test_req_id", "&'buf nexus_fix_codec::AsciiTextStr")],
    ),
    (
        "TestRequest",
        "1",
        &[(112, "test_req_id", "&'buf nexus_fix_codec::AsciiTextStr")],
    ),
    (
        "ResendRequest",
        "2",
        &[(7, "begin_seq_no", "u64"), (16, "end_seq_no", "u64")],
    ),
    (
        "SequenceReset",
        "4",
        &[(36, "new_seq_no", "u64"), (123, "gap_fill_flag", "bool")],
    ),
    (
        "Reject",
        "3",
        &[
            (45, "ref_seq_num", "u64"),
            (58, "text", "&'buf nexus_fix_codec::AsciiTextStr"),
        ],
    ),
];

pub fn emit(messages: &[RMessage]) -> String {
    let mut s = String::new();
    s.push_str(HEADER);
    for &(name, msgtype, proto_fields) in SPECS {
        emit_admin_type(&mut s, name, msgtype, proto_fields, messages);
    }
    s
}

fn extra_fields<'a>(
    messages: &'a [RMessage],
    msgtype: &str,
    proto_tags: &HashSet<u32>,
) -> Vec<&'a RField> {
    messages
        .iter()
        .find(|m| m.is_admin && m.msgtype == msgtype)
        .map(|msg| {
            msg.members
                .iter()
                .filter_map(|mem| match mem {
                    RMember::Field(f) if !proto_tags.contains(&f.number) => Some(f),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn emit_admin_type(
    s: &mut String,
    name: &str,
    msgtype: &str,
    proto_fields: &[ProtocolField],
    messages: &[RMessage],
) {
    let proto_tags: HashSet<u32> = proto_fields.iter().map(|&(tag, _, _)| tag).collect();
    let extra = extra_fields(messages, msgtype, &proto_tags);

    // Struct
    let _ = writeln!(s, "pub struct {name}<'buf> {{");
    s.push_str("    header: super::header::HeaderDecoder<'buf>,\n");
    for &(_, field_name, _) in proto_fields {
        let _ = writeln!(s, "    {field_name}: nexus_fix_codec::FieldSpan,");
    }
    for f in &extra {
        let _ = writeln!(s, "    {}: nexus_fix_codec::FieldSpan,", snake(&f.name));
    }
    s.push_str("    checksum: nexus_fix_codec::FieldSpan,\n}\n\n");

    // FixAdminMsg impl
    let _ = writeln!(
        s,
        "impl<'buf> nexus_fix_codec::FixAdminMsg<'buf> for {name}<'buf> {{"
    );
    s.push_str(
        "    fn decode(buf: &'buf [u8]) -> Result<Self, nexus_fix_codec::DecodeError> { Self::decode(buf) }\n}\n\n",
    );

    // inherent impl
    let _ = writeln!(s, "impl<'buf> {name}<'buf> {{");

    // wrap_unchecked
    s.push_str("    pub fn wrap_unchecked(header: super::header::HeaderDecoder<'buf>) -> Result<Self, nexus_fix_codec::DecodeError> {\n");
    s.push_str("        let mut m = Self {\n            header,\n");
    for &(_, field_name, _) in proto_fields {
        let _ = writeln!(
            s,
            "            {field_name}: nexus_fix_codec::FieldSpan::EMPTY,"
        );
    }
    for f in &extra {
        let _ = writeln!(
            s,
            "            {}: nexus_fix_codec::FieldSpan::EMPTY,",
            snake(&f.name)
        );
    }
    s.push_str("            checksum: nexus_fix_codec::FieldSpan::EMPTY,\n        };\n");
    s.push_str(
        "        while let Some(f) = m.header.reader.next_field() {\n            match f.tag {\n",
    );
    for &(tag, field_name, _) in proto_fields {
        let _ = writeln!(
            s,
            "                {tag} => {{ m.{field_name} = f.value; }}"
        );
    }
    for f in &extra {
        let _ = writeln!(
            s,
            "                {} => {{ m.{} = f.value; }}",
            f.number,
            snake(&f.name)
        );
    }
    s.push_str(
        "                10 => { m.checksum = f.value; break; }\n                _ => {}\n            }\n        }\n        Ok(m)\n    }\n\n",
    );

    // decode
    s.push_str(
        "    pub fn decode(buf: &'buf [u8]) -> Result<Self, nexus_fix_codec::DecodeError> {\n",
    );
    s.push_str(
        "        Self::wrap_unchecked(super::header::HeaderDecoder::decode(buf))\n    }\n\n",
    );

    // header()
    s.push_str(
        "    pub fn header(&self) -> &super::header::HeaderDecoder<'buf> { &self.header }\n\n",
    );

    // protocol field accessors
    for &(_, field_name, rust_type) in proto_fields {
        let _ = write!(
            s,
            "    pub fn {field_name}(&self) -> Option<nexus_fix_codec::FieldView<'buf, {rust_type}>> {{\n        \
             nexus_fix_codec::FieldView::new(self.{field_name}, self.header.reader.buf())\n    \
             }}\n\n"
        );
    }

    // venue-specific accessors
    for f in &extra {
        emit_value_accessor(s, f, "self.header.reader.buf()");
    }

    s.push_str("}\n\n");
}

/// The 7 associated type assignments for the `FixDictionary` impl.
pub fn emit_dict_assoc_types(s: &mut String) {
    for &(name, _, _) in SPECS {
        let _ = writeln!(s, "    type {name}<'buf> = admin::{name}<'buf>;");
    }
}
