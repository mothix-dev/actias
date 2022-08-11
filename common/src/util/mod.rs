pub mod array;

use core::{fmt, fmt::LowerHex};

pub struct FormatHex<T: LowerHex>(pub T);

impl<T: LowerHex> fmt::Debug for FormatHex<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#x}", self.0)
    }
}

pub struct DebugArray<'a, T: fmt::Debug>(pub &'a [T]);
const ARRAY_LIMIT: usize = 128;

impl<T: fmt::Debug> fmt::Debug for DebugArray<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.len() > ARRAY_LIMIT {
            write!(f, "[{}; {}]", core::any::type_name::<T>(), self.0.len())
        } else {
            write!(f, "{:?}", self.0)
        }
    }
}
