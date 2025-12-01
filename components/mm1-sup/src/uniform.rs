use crate::common::child_spec::ChildSpec;
mod sup_actor;
mod sup_context;

pub use sup_actor::uniform_sup;
pub use sup_context::UniformSupContext;

pub mod child_type {
    use mm1_common::types::AnyError;
    use tokio::time::Instant;

    use crate::common::factory::ActorFactory;
    use crate::common::restart_intensity::{RestartIntensity, RestartStats};

    pub trait UniformChildType<A> {
        type Data;

        fn new_data(&self, args: A) -> Self::Data;
        fn make_runnable<F>(
            &self,
            factory: &F,
            data: &mut Self::Data,
        ) -> Result<F::Runnable, AnyError>
        where
            F: ActorFactory<Args = A>;
        fn should_restart(
            &self,
            data: &mut Self::Data,
            normal_exit: bool,
        ) -> Result<bool, AnyError>;
    }

    #[derive(Debug, Clone, Copy)]
    pub struct Temporary;

    pub type Permanent = Restarting<true>;

    pub type Transient = Restarting<false>;

    impl<A> UniformChildType<A> for Temporary {
        type Data = Option<A>;

        fn new_data(&self, args: A) -> Self::Data {
            Some(args)
        }

        fn make_runnable<F>(
            &self,
            factory: &F,
            data: &mut Self::Data,
        ) -> Result<F::Runnable, AnyError>
        where
            F: ActorFactory<Args = A>,
        {
            let args = data.take().ok_or_else(|| eyre::format_err!("args gone"))?;
            let runnable = factory.produce(args);
            Ok(runnable)
        }

        fn should_restart(
            &self,
            _data: &mut Self::Data,
            _normal_exit: bool,
        ) -> Result<bool, AnyError> {
            Ok(false)
        }
    }

    #[doc(hidden)]
    pub struct Restarting<const RESTART_ON_NORMAL_EXIT: bool> {
        restart_intensity: RestartIntensity,
    }

    impl<const RESTART_ON_NORMAL_EXIT: bool, A> UniformChildType<A>
        for Restarting<RESTART_ON_NORMAL_EXIT>
    where
        A: Clone,
    {
        type Data = ChildData<A>;

        fn new_data(&self, args: A) -> Self::Data {
            let restarts = self.restart_intensity.new_stats();
            ChildData { args, restarts }
        }

        fn make_runnable<F>(
            &self,
            factory: &F,
            data: &mut Self::Data,
        ) -> Result<F::Runnable, AnyError>
        where
            F: ActorFactory<Args = A>,
        {
            let args = data.args.clone();
            let runnable = factory.produce(args);
            Ok(runnable)
        }

        fn should_restart(
            &self,
            data: &mut Self::Data,
            _normal_exit: bool,
        ) -> Result<bool, AnyError> {
            self.restart_intensity
                .report_exit(&mut data.restarts, Instant::now())?;
            Ok(RESTART_ON_NORMAL_EXIT)
        }
    }

    #[doc(hidden)]
    pub struct ChildData<A> {
        args:     A,
        restarts: RestartStats,
    }
}

pub struct UniformSup<F, C> {
    pub child_spec: ChildSpec<F, C>,
}

impl<F, C> UniformSup<F, C> {
    pub fn new(child_spec: ChildSpec<F, C>) -> Self {
        Self { child_spec }
    }
}

impl<F, C> Clone for UniformSup<F, C>
where
    ChildSpec<F, C>: Clone,
{
    fn clone(&self) -> Self {
        Self {
            child_spec: self.child_spec.clone(),
        }
    }
}
