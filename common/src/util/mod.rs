pub mod array;

use core::{fmt, fmt::LowerHex};

pub struct FormatHex<T: LowerHex>(pub T);

impl<T: LowerHex> fmt::Debug for FormatHex<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#x}", self.0)
    }
}
