use std::ops::{Deref, DerefMut};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Local<T>(pub T);

impl<T> Local<T> {
    pub fn into_inner(self) -> T {
        self.0
    }
}

impl<T> From<T> for Local<T> {
    fn from(value: T) -> Self {
        Local(value)
    }
}

impl<T> Deref for Local<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for Local<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_work() {
        fn ensure_is_a_message<M: crate::Message>(_: &M) {}
        let one = Local(1u64);
        ensure_is_a_message(&one);
        let not_one = one.reverse_bits();
        assert_ne!(*one, not_one);
    }
}
