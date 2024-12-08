// #![warn(missing_docs)]
#![warn(rust_2018_idioms)]
#![warn(unreachable_pub)]

pub mod proto {
    pub use {mm1_proto as common, mm1_proto_sup as sup, mm1_proto_system as system};
}

pub mod address {
    pub use mm1_address::address::{Address, AddressParseError};
    pub use mm1_address::subnet::{
        InvalidMask, MaskParseError, NetAddress, NetAddressParseError, NetMask,
    };
}

pub mod common {
    pub use mm1_common::types::Never;

    pub mod log {
        pub use mm1_common::log::*;
    }

    pub mod error {
        pub use mm1_common::errors::error_kind::HasErrorKind;
        pub use mm1_common::errors::error_of::ErrorOf;
        pub use mm1_common::types::{AnyError, StdError};
    }

    pub mod future {
        pub use mm1_common::futures::catch_panic::{CatchPanic, CatchPanicExt};
        pub use mm1_common::futures::timeout::FutureTimeoutExt;
    }
}

pub mod core {
    pub mod message {
        pub use mm1_core::prim::{Local, Unique};
    }

    pub mod envelope {
        pub use mm1_core::envelope::{dispatch, Envelope, EnvelopeInfo};
    }

    pub mod context {
        pub use mm1_core::context::{
            Ask, AskErrorKind, Call, Fork, ForkErrorKind, InitDone, Linking, Quit, Recv,
            RecvErrorKind, ShutdownErrorKind, Start, Stop, Tell, TellErrorKind, TryCall, Watching,
        };
    }
}

pub mod sup {
    pub mod common {
        use std::time::Duration;

        pub use mm1_sup::common::child_spec::{ChildSpec, ChildTimeouts, ChildType, InitType};
        pub use mm1_sup::common::factory::{ActorFactory, ActorFactoryMut};

        pub type RestartIntensity = mm1_sup::common::restart_intensity::RestartIntensity<Duration>;
        pub use mm1_sup::common::restart_intensity::MaxRestartIntensityReached;
    }

    pub mod uniform {
        pub use mm1_sup::uniform::{uniform_sup, UniformSup, UniformSupFailure};
    }
}

pub mod runtime {
    pub use mm1_node::runtime::{config, Local, Remote, Rt};
}
