/// A (offset, length) pair pointing into a FIX message buffer.
///
/// 8 bytes. All field access goes through this — the accessor reads
/// `buffer[span.offset..][..span.len]`. `u32` length accommodates
/// DATA-type fields which can exceed 64KB.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct FieldSpan {
    pub offset: u32,
    pub len: u32,
}

impl FieldSpan {
    pub const EMPTY: Self = Self { offset: 0, len: 0 };

    #[inline]
    pub const fn new(offset: u32, len: u32) -> Self {
        Self { offset, len }
    }

    #[inline]
    pub const fn is_present(&self) -> bool {
        self.len > 0
    }

    /// # Panics
    ///
    /// Panics if the span extends past the end of `buf`.
    #[inline]
    pub fn slice<'a>(&self, buf: &'a [u8]) -> &'a [u8] {
        &buf[self.offset as usize..][..self.len as usize]
    }
}

/// Location and count of a repeating group within a FIX message buffer.
///
/// 8 bytes (2 bytes padding after `count`), `Copy`. No allocation —
/// this is just a bookmark into the original message buffer.
///
/// The generated scanner populates this by walking fields via
/// [`FieldReader`](crate::FieldReader): when it encounters a count
/// tag (e.g. tag 268 = NoMDEntries), it records the buffer offset of
/// the first entry and iterates through entries — each starting with
/// the dictionary-defined delimiter tag — until a non-group tag
/// appears, then stores the offset and count. Accessing the group
/// later creates a new `FieldReader` at that offset and walks
/// `count` entries over the same buffer.
///
/// Entries within a group can have varying numbers of fields (some
/// are optional per the dictionary). The delimiter tag marks entry
/// boundaries; the set of valid group tags (from the dictionary)
/// determines where the group ends.
///
/// # Example
///
/// ```
/// use nexus_fix_codec::{FieldReader, GroupSpan};
///
/// // Two MDEntries: entry 1 has 3 fields, entry 2 has 2 fields.
/// let msg = b"268=2\x01269=0\x01270=50000\x01271=1\x01269=1\x01270=49999\x01";
///
/// let delimiter_tag = 269u32;
/// let group_tags: &[u32] = &[269, 270, 271];
///
/// // 1. Scanner finds count tag and records where entries start.
/// let mut reader = FieldReader::new(msg, 0);
/// let count_field = reader.next_field().unwrap();
/// assert_eq!(count_field.tag, 268);
/// let group = GroupSpan::new(reader.pos() as u32, 2);
///
/// // 2. Read the group from its offset, using dictionary knowledge
/// //    of valid group tags to detect where the group ends.
/// let mut reader = FieldReader::new(msg, group.offset as usize);
/// let mut entry_count = 0u16;
/// while let Some(field) = reader.next_field() {
///     if !group_tags.contains(&field.tag) {
///         break;
///     }
///     if field.tag == delimiter_tag {
///         entry_count += 1;
///     }
/// }
/// assert_eq!(entry_count, group.count);
/// ```
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct GroupSpan {
    pub offset: u32,
    pub count: u16,
}

impl GroupSpan {
    pub const EMPTY: Self = Self {
        offset: 0,
        count: 0,
    };

    #[inline]
    pub const fn new(offset: u32, count: u16) -> Self {
        Self { offset, count }
    }

    #[inline]
    pub const fn is_present(&self) -> bool {
        self.count > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_not_present() {
        assert!(!FieldSpan::EMPTY.is_present());
    }

    #[test]
    fn new_is_present() {
        let span = FieldSpan::new(10, 5);
        assert!(span.is_present());
        assert_eq!(span.offset, 10);
        assert_eq!(span.len, 5);
    }

    #[test]
    fn slice_extracts_correctly() {
        let buf = b"8=FIX.4.4\x0135=D\x01";
        let span = FieldSpan::new(2, 7);
        assert_eq!(span.slice(buf), b"FIX.4.4");
    }

    #[test]
    fn size_is_8_bytes() {
        assert_eq!(size_of::<FieldSpan>(), 8);
    }

    #[test]
    fn group_empty_is_not_present() {
        assert!(!GroupSpan::EMPTY.is_present());
    }

    #[test]
    fn group_new_is_present() {
        let g = GroupSpan::new(100, 3);
        assert!(g.is_present());
        assert_eq!(g.offset, 100);
        assert_eq!(g.count, 3);
    }

    #[test]
    fn group_size_is_8_bytes() {
        assert_eq!(size_of::<GroupSpan>(), 8);
    }
}
