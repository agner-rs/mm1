use std::sync::Arc;

use crate::common::factory::ActorFactory;
use crate::mixed::{ChildType, ErasedActorFactory, MixedSup};
type ChildSpec<F> = crate::common::child_spec::ChildSpec<F, ChildType>;

pub trait Append<K, F> {
    type Out;
    fn append(self, key: K, child_spec: ChildSpec<F>) -> Self::Out;
}

pub trait CollectInto<K, R> {
    fn collect_into(self, into: &mut impl Extend<(K, ChildSpec<ErasedActorFactory<R>>)>);
}

pub struct KV<K, R> {
    key:        K,
    child_spec: ChildSpec<ErasedActorFactory<R>>,
}

impl<K, F, T> Append<K, F> for T
where
    F: ActorFactory<Args = ()>,
{
    type Out = (Self, KV<K, F::Runnable>);

    fn append(self, key: K, child_spec: ChildSpec<F>) -> Self::Out {
        let child_spec = child_spec.map_launcher::<ErasedActorFactory<_>, _>(|f| Arc::pin(f));
        (self, KV { key, child_spec })
    }
}

impl<K, R> CollectInto<K, R> for () {
    fn collect_into(self, _into: &mut impl Extend<(K, ChildSpec<ErasedActorFactory<R>>)>) {}
}
impl<K, R> CollectInto<K, R> for KV<K, R> {
    fn collect_into(self, into: &mut impl Extend<(K, ChildSpec<ErasedActorFactory<R>>)>) {
        let Self { key, child_spec } = self;
        into.extend([(key, child_spec)]);
    }
}
impl<K, R, Le, Ri> CollectInto<K, R> for (Le, Ri)
where
    Le: CollectInto<K, R>,
    Ri: CollectInto<K, R>,
{
    fn collect_into(self, into: &mut impl Extend<(K, ChildSpec<ErasedActorFactory<R>>)>) {
        let (le, ri) = self;
        le.collect_into(into);
        ri.collect_into(into);
    }
}

impl<RS, C> MixedSup<RS, C> {
    pub fn with_child<K, F>(
        self,
        key: K,
        child_spec: ChildSpec<F>,
    ) -> MixedSup<RS, <C as Append<K, F>>::Out>
    where
        C: Append<K, F>,
    {
        let Self {
            restart_strategy,
            children,
        } = self;
        let children = children.append(key, child_spec);
        MixedSup {
            restart_strategy,
            children,
        }
    }
}

impl<RS, C> Clone for MixedSup<RS, C>
where
    RS: Clone,
    C: Clone,
{
    fn clone(&self) -> Self {
        Self {
            restart_strategy: self.restart_strategy.clone(),
            children:         self.children.clone(),
        }
    }
}
