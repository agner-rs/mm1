use std::collections::{HashMap, HashSet};
use std::fmt;
use std::hash::Hash;
use std::sync::Arc;

use crate::codec::{Outcome, Process, RequiredType, SupportedTypes};

#[derive(thiserror::Error)]
pub enum DispatcherBuildError<K: fmt::Debug, T> {
    #[error("duplicate key: {:?}", _0)]
    DuplicateKey(K, T),
}

pub struct Dispatcher<R, S, K, I, O, E> {
    selector: R,
    backends: HashMap<K, Arc<dyn Process<I, State = S, Output = O, Error = E>>>,
}

impl<R, S, K, I, O, E> Dispatcher<R, S, K, I, O, E>
where
    K: Eq + Hash + fmt::Debug,
{
    pub fn new(selector: R) -> Self {
        Self {
            selector,
            backends: Default::default(),
        }
    }

    pub fn add<P>(&mut self, backend: P) -> Result<&mut Self, DispatcherBuildError<K, P>>
    where
        P: SupportedTypes<K>,
        P: Process<I, State = S, Output = O, Error = E> + 'static,
    {
        let mut keys = backend.supported_types().collect::<HashSet<_>>();
        if let Some(duplicate_key) = self.backends.keys().find_map(|k| keys.take(k)) {
            return Err(DispatcherBuildError::DuplicateKey(duplicate_key, backend))
        }

        let erased: Arc<dyn Process<I, State = S, Output = O, Error = E>> = Arc::new(backend);

        for key in keys {
            self.backends.insert(key, Arc::clone(&erased));
        }

        Ok(self)
    }

    pub fn with<P>(mut self, backend: P) -> Result<Self, DispatcherBuildError<K, P>>
    where
        P: SupportedTypes<K>,
        P: Process<I, State = S, Output = O, Error = E> + 'static,
    {
        self.add(backend)?;
        Ok(self)
    }
}

impl<R, S, K, I, O, E> Process<I> for Dispatcher<R, S, K, I, O, E>
where
    K: Eq + Hash,
    R: RequiredType<K, I>,
{
    type Error = E;
    type Output = O;
    type State = S;

    fn process(
        &self,
        state: &mut Self::State,
        input: I,
    ) -> Result<Outcome<Self::Output, I>, Self::Error> {
        let Some(key) = self.selector.required_type(&input) else {
            return Ok(Outcome::Rejected(input))
        };
        let Some(backend) = self.backends.get(&key) else {
            return Ok(Outcome::Rejected(input))
        };
        backend.process(state, input)
    }
}

impl<K: fmt::Debug, T> fmt::Debug for DispatcherBuildError<K, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateKey(k, _) => write!(f, "DuplicateKey({:?})", k),
        }
    }
}

#[cfg(test)]
mod ergonomics {
    use std::any::TypeId;
    use std::convert::Infallible;

    use super::*;
    use crate::codec::{Outcome, Process};

    struct X;
    impl RequiredType<TypeId, E> for X {
        fn required_type(&self, data: &E) -> Option<TypeId> {
            Some(data.0)
        }
    }
    impl RequiredType<TypeId, P> for X {
        fn required_type(&self, data: &P) -> Option<TypeId> {
            Some(data.0)
        }
    }

    struct M1;
    struct M2;
    struct M3;

    struct P(TypeId);

    struct S;

    struct E(TypeId);

    struct C1;
    struct C2;

    impl Process<P> for C1 {
        type Error = Infallible;
        type Output = E;
        type State = S;

        fn process(
            &self,
            S: &mut Self::State,
            packet: P,
        ) -> Result<Outcome<Self::Output, P>, Self::Error> {
            Ok(if packet.0 == TypeId::of::<M1>() {
                Outcome::Done(E(packet.0))
            } else {
                Outcome::Rejected(packet)
            })
        }
    }

    impl Process<P> for C2 {
        type Error = Infallible;
        type Output = E;
        type State = S;

        fn process(
            &self,
            S: &mut Self::State,
            packet: P,
        ) -> Result<Outcome<Self::Output, P>, Self::Error> {
            Ok(if packet.0 == TypeId::of::<M2>() {
                Outcome::Done(E(packet.0))
            } else {
                Outcome::Rejected(packet)
            })
        }
    }

    impl SupportedTypes<TypeId> for C1 {
        fn supported_types(&self) -> impl Iterator<Item = TypeId> {
            [std::any::TypeId::of::<M1>()].into_iter()
        }
    }

    impl SupportedTypes<TypeId> for C2 {
        fn supported_types(&self) -> impl Iterator<Item = TypeId> {
            [std::any::TypeId::of::<M2>()].into_iter()
        }
    }

    #[test]
    fn ergonomics_01() {
        let mut state = S;

        let mut d = Dispatcher::new(X)
            .with(C1)
            .expect("couldn't add C1")
            .with(C2)
            .expect("couldn't add C2");

        d.add(C1)
            .map(|_| ())
            .expect_err("couldn't have added C1 again");

        assert!(matches!(
            C1.process(&mut state, P(TypeId::of::<M1>())).unwrap(),
            Outcome::Done(E(_))
        ));
        assert!(matches!(
            C1.process(&mut state, P(TypeId::of::<M2>())).unwrap(),
            Outcome::Rejected(P(_))
        ));
        assert!(matches!(
            C1.process(&mut state, P(TypeId::of::<M3>())).unwrap(),
            Outcome::Rejected(P(_))
        ));

        assert!(matches!(
            C2.process(&mut state, P(TypeId::of::<M1>())).unwrap(),
            Outcome::Rejected(P(_))
        ));
        assert!(matches!(
            C2.process(&mut state, P(TypeId::of::<M2>())).unwrap(),
            Outcome::Done(E(_))
        ));
        assert!(matches!(
            C2.process(&mut state, P(TypeId::of::<M3>())).unwrap(),
            Outcome::Rejected(P(_))
        ));

        assert!(matches!(
            d.process(&mut state, P(TypeId::of::<M1>())).unwrap(),
            Outcome::Done(E(_))
        ));
        assert!(matches!(
            d.process(&mut state, P(TypeId::of::<M2>())).unwrap(),
            Outcome::Done(E(_))
        ));
        assert!(matches!(
            d.process(&mut state, P(TypeId::of::<M3>())).unwrap(),
            Outcome::Rejected(P(_))
        ));
    }
}
