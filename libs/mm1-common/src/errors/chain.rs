use std::error::Error as StdError;
use std::fmt;

use crate::types::AnyError;

pub trait ExactTypeDisplayChainExt {
    fn as_display_chain(&self) -> impl fmt::Display + fmt::Debug;
}

pub trait StdErrorDisplayChainExt: StdError + Sized {
    fn as_display_chain(&self) -> impl fmt::Display + fmt::Debug {
        let e: &dyn StdError = self;
        D(e)
    }
}

impl<E> StdErrorDisplayChainExt for E where E: StdError {}

impl<T> ExactTypeDisplayChainExt for T
where
    for<'a> D<&'a T>: fmt::Display + fmt::Debug,
{
    fn as_display_chain(&self) -> impl fmt::Display + fmt::Debug {
        D(self)
    }
}

struct D<T>(T);

impl fmt::Display for D<&dyn StdError> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut e = self.0;
        write!(f, "{}", e)?;
        while let Some(source) = e.source() {
            write!(f, " << {}", source)?;
            e = source;
        }
        Ok(())
    }
}
impl fmt::Debug for D<&dyn StdError> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl fmt::Display for D<&AnyError> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut chain = self.0.chain();
        if let Some(e) = chain.next() {
            write!(f, "{}", e)?;
        }
        for source in chain {
            write!(f, "<< {}", source)?;
        }

        Ok(())
    }
}

impl fmt::Debug for D<&AnyError> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
