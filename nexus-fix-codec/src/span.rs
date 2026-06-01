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
/// 8 bytes (2 bytes padding after `count`). Stored in the generated
/// flyweight decoder to track where a group starts and how many
/// entries it contains.
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
