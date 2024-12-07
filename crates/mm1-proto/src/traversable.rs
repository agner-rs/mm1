pub use mm1_common::types::AnyError;
pub use mm1_proc_macros::Traversable;

pub trait Traversable<Inspector, Adjuster> {
    type InspectError: Into<crate::AnyError>;
    type AdjustError: Into<crate::AnyError>;

    fn inspect(&self, visitor: &mut Inspector) -> Result<(), Self::InspectError>;
    fn adjust(&mut self, visitor: &mut Adjuster) -> Result<(), Self::AdjustError>;
}

mod well_known_types {
    use std::convert::Infallible;

    use either::Either;

    use crate::Traversable;

    macro_rules! primitives {
        ( $( $t:ty ),* $(,)? ) => {
            $(
                impl<I,A> Traversable<I,A> for $t {
                    type InspectError = Infallible;
                    type AdjustError = Infallible;
                    fn inspect(&self, _visitor: &mut I) -> Result<(), Self::InspectError> { Ok(()) }
                    fn adjust(&mut self, _visitor: &mut A) -> Result<(), Self::AdjustError> { Ok(()) }
                }
            )*
        };
    }
    primitives!(
        (),
        u8,
        u16,
        u32,
        u64,
        u128,
        i8,
        i16,
        i32,
        i64,
        i128,
        isize,
        usize,
        char,
        bool,
        f32,
        f64
    );

    impl<T, const S: usize, I, A> Traversable<I, A> for [T; S]
    where
        T: Traversable<I, A>,
    {
        type AdjustError = T::AdjustError;
        type InspectError = T::InspectError;

        fn inspect(&self, visitor: &mut I) -> Result<(), Self::InspectError> {
            self.iter().try_for_each(|item| item.inspect(visitor))
        }

        fn adjust(&mut self, visitor: &mut A) -> Result<(), Self::AdjustError> {
            self.iter_mut().try_for_each(|item| item.adjust(visitor))
        }
    }

    impl<T, I, A> Traversable<I, A> for Vec<T>
    where
        T: Traversable<I, A>,
    {
        type AdjustError = T::AdjustError;
        type InspectError = T::InspectError;

        fn inspect(&self, visitor: &mut I) -> Result<(), Self::InspectError> {
            self.iter().try_for_each(|item| item.inspect(visitor))
        }

        fn adjust(&mut self, visitor: &mut A) -> Result<(), Self::AdjustError> {
            self.iter_mut().try_for_each(|item| item.adjust(visitor))
        }
    }

    impl<T, E, I, A> Traversable<I, A> for Result<T, E>
    where
        T: Traversable<I, A>,
        E: Traversable<I, A>,
    {
        type AdjustError = crate::AnyError;
        type InspectError = crate::AnyError;

        fn inspect(&self, visitor: &mut I) -> Result<(), Self::InspectError> {
            self.as_ref()
                .map(|value| value.inspect(visitor).map_err(Into::into))
                .map_err(|error| error.inspect(visitor).map_err(Into::into))
                .unwrap_or_else(std::convert::identity)
        }

        fn adjust(&mut self, visitor: &mut A) -> Result<(), Self::AdjustError> {
            self.as_mut()
                .map(|value| value.adjust(visitor).map_err(Into::into))
                .map_err(|error| error.adjust(visitor).map_err(Into::into))
                .unwrap_or_else(std::convert::identity)
        }
    }

    impl<T, I, A> Traversable<I, A> for Option<T>
    where
        T: Traversable<I, A>,
    {
        type AdjustError = T::AdjustError;
        type InspectError = T::InspectError;

        fn inspect(&self, visitor: &mut I) -> Result<(), Self::InspectError> {
            self.as_ref()
                .map(|value| value.inspect(visitor))
                .unwrap_or(Ok(()))
        }

        fn adjust(&mut self, visitor: &mut A) -> Result<(), Self::AdjustError> {
            self.as_mut()
                .map(|value| value.adjust(visitor))
                .unwrap_or(Ok(()))
        }
    }

    impl<L, R, I, A> Traversable<I, A> for Either<L, R>
    where
        L: Traversable<I, A>,
        R: Traversable<I, A>,
    {
        type AdjustError = crate::AnyError;
        type InspectError = crate::AnyError;

        fn inspect(&self, visitor: &mut I) -> Result<(), Self::InspectError> {
            self.as_ref()
                .map_left(|l| l.inspect(visitor).map_err(Into::into))
                .map_right(|r| r.inspect(visitor).map_err(Into::into))
                .into_inner()
        }

        fn adjust(&mut self, visitor: &mut A) -> Result<(), Self::AdjustError> {
            self.as_mut()
                .map_left(|l| l.adjust(visitor).map_err(Into::into))
                .map_right(|r| r.adjust(visitor).map_err(Into::into))
                .into_inner()
        }
    }
}
