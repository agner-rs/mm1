// #![warn(missing_docs)]
#![warn(rust_2018_idioms)]
#![warn(unreachable_pub)]

pub mod proto {
    //! Protocols. Actors interact by communication.

    /// A trait showing that the type implementing it can be sent between the
    /// actors.
    pub use mm1_proto::Message;
    /// A proc-macro attribute to make a message out of a type.
    ///
    /// Example:
    /// ```rust
    /// #[message]
    /// struct Accept {
    ///     reply_to: Address,
    ///     timeout:  Duration,
    /// }
    ///
    /// #[message]
    /// struct Accepted {
    ///     io: Unique<TcpStream>,
    /// }
    /// ```
    pub use mm1_proto::message;
    #[cfg(feature = "sup")]
    /// The protocol to communicate with supervisors.
    /// See [`sup`](crate::sup).
    pub use mm1_proto_sup as sup;
    /// The low-level API-to the actor system.
    /// See [`Call`](crate::core::context::Call).
    pub use mm1_proto_system as system;
}

pub mod address {
    //! Addresses, masks, subnets.
    //!
    //! Addresses in `mm1` are used as destinations to send messages to.
    //! A type `Address` is represented as a `u64` integer, and is very similar
    //! to IPv4- or IPv6-address, in that the whole space of addresses may be
    //! split into sub-spaces using netmasks.
    //!
    //! > Example:
    //! >
    //! > A subnet `aabbccddee000000/40` contains 2^24 addresses: from
    //! > `aabbccddee000000` to `aabbccddeeFFFFFF`.
    //!
    //! The way addresses are written takes page from IPv6's notation: the
    //! leftmost longest series of consequent zero hex-digits is replaced with a
    //! `':'`-sign. To improve readability, the address is surrounded by
    //! corner brackets.
    //!
    //! The reasons to choose corner brackets:
    //! - so that they don't mix visually with IPv6-addresses.
    //! - so that they do not require additional quotes when used in YAML.
    //! - so that the addresses remind us a little bit of Erlang PIDs :).
    //!
    //! > Example:
    //! > - `aabbccddee000000/40` shall be written as `<aabbccddee:>/40`.
    //! > - `ffff000000084b03/64` shall be written as `<ffff:84b03>/64`.
    //!
    //! Actors' implementations should treat addresses as opaque types
    //! (implementing `Message`, `Copy`, `Eq`, `Cmp`, and `Hash`).
    //!
    //! The nature of addresses should serve the convenience of the operators,
    //! and probably ease up the implementation of the multi-node messaging.
    //!
    //! So, the default value for the node's subnet is `<ffff:>/16`. This
    //! probably should be treated as `127.0.0.0/8` in IPv4.

    /// Address — a destination to send messages to.
    pub use mm1_address::address::Address;
    pub use mm1_address::address::AddressParseError;
    pub use mm1_address::pool::{
        Lease as AddressLease, LeaseError as AddressLeaseError, Pool as AddressPool,
    };
    /// Address of a network, i.e. an `Address` in combination with a `NetMask`.
    pub use mm1_address::subnet::NetAddress;
    /// Mask — specifies how many leading bits in the address are fixed.
    pub use mm1_address::subnet::NetMask;
    pub use mm1_address::subnet::{InvalidMask, MaskParseError, NetAddressParseError};
}

pub mod common {

    /// A helper to define an error-kind enum
    pub use mm1_common::impl_error_kind;
    /// An empty type, i.e. no instance of that type can be produced.
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
    //! The API to implement actors.

    pub use mm1_core::tracing;

    pub mod envelope {
        //! An [`Envelope`] is a type-erasing container for the sent messages.

        /// A type-erasing container for the message that has been sent.
        pub use mm1_core::envelope::Envelope;
        /// An opaque type containing some information about the message that
        /// has been sent.
        pub use mm1_core::envelope::EnvelopeHeader;
        /// A macro helping to match an [`Envelope`].
        pub use mm1_core::envelope::dispatch;
    }

    pub mod context {
        //! Actor's behaviour is defined as an async-function that receives an
        //! exclusive reference to some *context* as its first argument.
        //! The concrete type of the *context* is supposed to remain unknown to
        //! the actors: they are to interact with their *contexts* via a set of
        //! traits, that are defined on a *context*.

        /// Report the completion of the init-phase.
        pub use mm1_core::context::InitDone;
        /// Link to/unlink from other actors.
        pub use mm1_core::context::Linking;
        /// Provides a current Instant.
        pub use mm1_core::context::Now;
        /// Terminate its own execution.
        pub use mm1_core::context::Quit;
        /// Start other actors.
        pub use mm1_core::context::Start;
        /// A convenience trait to simply send a message to another actor.
        pub use mm1_core::context::Tell;
        /// Watch/unwatch the termination of other actors.
        pub use mm1_core::context::Watching;
        /// Create another context (probably of a different kind), having
        /// "bound" to the required address. The exact type of address,
        /// and the concept of "bind" are (intentionally) defined a bit loosely.
        /// Examples:
        /// - `Bind<NetAddress>` — would most likely mean catch all the messages
        ///   sent to any address within the specified network;
        /// - `Bind<TypeId>` — handle the requests of certain kind.
        pub use mm1_core::context::{Bind, BindArgs, BindErrorKind};
        /// Create another context, having an address distinct from the original
        /// context's one.
        pub use mm1_core::context::{Fork, ForkErrorKind};
        /// Send and receive messages.
        pub use mm1_core::context::{Messaging, RecvErrorKind, SendErrorKind};
        /// Stop other actors.
        pub use mm1_core::context::{ShutdownErrorKind, Stop};
    }
}

#[cfg(feature = "ask")]
pub mod ask {
    pub use mm1_ask::{Ask, AskErrorKind, Reply};

    pub mod proto {
        pub type RequestHeader = mm1_proto_ask::RequestHeader;
        pub type Request<Rq> = mm1_proto_ask::Request<Rq>;

        #[doc(hidden)]
        pub type ResponseHeader = mm1_proto_ask::ResponseHeader;
        #[doc(hidden)]
        pub type Response<Rs> = mm1_proto_ask::Response<Rs>;
    }
}

#[cfg(feature = "sup")]
pub mod sup {
    //! Supervisors — the actors that manage other actors.

    pub mod common {
        //! The building blocks shared across different types of supervisors.

        /// A recipe for a child-actor.
        pub use mm1_sup::common::child_spec::ChildSpec;
        pub use mm1_sup::common::child_spec::{ChildType, InitType};
        pub use mm1_sup::common::factory::ActorFactory;
        /// Multiple-use actor factory.
        pub use mm1_sup::common::factory::ActorFactoryMut;
        /// Single-use actor-factory.
        pub use mm1_sup::common::factory::ActorFactoryOnce;

        pub type RestartIntensity = mm1_sup::common::restart_intensity::RestartIntensity;

        pub use mm1_sup::common::restart_intensity::MaxRestartIntensityReached;
    }

    pub mod uniform {
        //! Uniform supervisor — the actor, that supervises the children of the
        //! same type.

        /// The recipe for a supervisor.
        pub use mm1_sup::uniform::UniformSup;
        /// Blanket trait for contexts suitable for running a
        /// uniform-supervisor.
        pub use mm1_sup::uniform::UniformSupContext;
        /// The behaviour function of the uniform supervisor actor.
        pub use mm1_sup::uniform::uniform_sup;
    }

    pub mod mixed {
        //! Mixed supervisor — the actor, that supervises the children of
        //! different types.

        /// The recipe for a supervisor.
        pub use mm1_sup::mixed::MixedSup;
        /// Blanket trait for contexts suitable for running a mixed-supervisor.
        pub use mm1_sup::mixed::MixedSupContext;
        /// Mixed supervisor's failure type.
        pub use mm1_sup::mixed::MixedSupError;
        /// The behaviour function of the mixed supervisor actor.
        pub use mm1_sup::mixed::mixed_sup;
        /// module with supervision strategies.
        pub use mm1_sup::mixed::strategy;
    }
}

#[cfg(feature = "runtime")]
pub mod runtime {
    pub use mm1_node::config;
    pub use mm1_node::runtime::{Local, Rt};
}

pub use mm1_runnable as runnable;

#[cfg(feature = "timer")]
pub mod timer {
    pub use mm1_timer::v1;
}

#[cfg(feature = "multinode")]
pub mod multinode {
    pub mod proto {
        pub use mm1_proto_network_management::protocols::{
            RegisterProtocolRequest, RegisterProtocolResponse,
        };
    }

    pub use mm1_multinode::codec::Protocol;
    pub use mm1_proto_well_known::MULTINODE_MANAGER;
}

// #[cfg(feature = "multinode")]
// pub mod message_codec {
//     pub use mm1_multinode::codecs::Codec;
// }

#[cfg(feature = "test-util")]
pub mod test {
    pub use mm1_test_rt::rt;
}
