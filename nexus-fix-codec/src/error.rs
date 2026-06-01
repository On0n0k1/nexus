use core::fmt;

/// Error during FIX message decoding.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// Message is too short to contain required header fields.
    Truncated,
    /// No `=` separator found in a tag=value field.
    MissingSeparator,
    /// Tag number is zero or contains non-digit bytes.
    InvalidTag,
    /// BeginString (tag 8) is missing or not the first field.
    MissingBeginString,
    /// BodyLength (tag 9) is missing or not the second field.
    MissingBodyLength,
    /// Checksum validation failed.
    Checksum(ChecksumError),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => f.write_str("message truncated"),
            Self::MissingSeparator => f.write_str("missing '=' separator in field"),
            Self::InvalidTag => f.write_str("invalid tag number"),
            Self::MissingBeginString => f.write_str("missing or misplaced BeginString (tag 8)"),
            Self::MissingBodyLength => f.write_str("missing or misplaced BodyLength (tag 9)"),
            Self::Checksum(e) => write!(f, "checksum: {}", e),
        }
    }
}

impl std::error::Error for DecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Checksum(e) => Some(e),
            _ => None,
        }
    }
}

impl From<ChecksumError> for DecodeError {
    fn from(e: ChecksumError) -> Self {
        Self::Checksum(e)
    }
}

/// Checksum validation failure.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ChecksumError {
    pub expected: u8,
    pub computed: u8,
}

impl fmt::Display for ChecksumError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "expected {:03}, computed {:03}",
            self.expected, self.computed
        )
    }
}

impl std::error::Error for ChecksumError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_error_display() {
        let e = ChecksumError {
            expected: 178,
            computed: 42,
        };
        assert_eq!(e.to_string(), "expected 178, computed 042");
    }

    #[test]
    fn decode_error_from_checksum() {
        let ce = ChecksumError {
            expected: 1,
            computed: 2,
        };
        let de: DecodeError = ce.into();
        assert_eq!(de, DecodeError::Checksum(ce));
    }

    #[test]
    fn decode_error_display() {
        assert_eq!(DecodeError::Truncated.to_string(), "message truncated");
        assert_eq!(
            DecodeError::MissingSeparator.to_string(),
            "missing '=' separator in field"
        );
        assert_eq!(DecodeError::InvalidTag.to_string(), "invalid tag number");
        assert_eq!(
            DecodeError::MissingBeginString.to_string(),
            "missing or misplaced BeginString (tag 8)"
        );
        assert_eq!(
            DecodeError::MissingBodyLength.to_string(),
            "missing or misplaced BodyLength (tag 9)"
        );
        let ce = ChecksumError {
            expected: 178,
            computed: 42,
        };
        assert_eq!(
            DecodeError::Checksum(ce).to_string(),
            "checksum: expected 178, computed 042"
        );
    }

    #[test]
    fn decode_error_source_chain() {
        use std::error::Error;

        assert!(DecodeError::Truncated.source().is_none());
        assert!(DecodeError::MissingSeparator.source().is_none());
        assert!(DecodeError::InvalidTag.source().is_none());

        let ce = ChecksumError {
            expected: 1,
            computed: 2,
        };
        let de = DecodeError::Checksum(ce);
        let src = de.source().unwrap();
        let downcasted = src.downcast_ref::<ChecksumError>().unwrap();
        assert_eq!(*downcasted, ce);
    }
}
