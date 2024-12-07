// #![warn(missing_docs)]
#![warn(rust_2018_idioms)]
#![warn(unreachable_pub)]

pub mod proto {
    pub use {mm1_proto as common, mm1_proto_sup as sup, mm1_proto_system as system};
}

pub mod runtime {
    pub use mm1_node::runtime::{config, Local, Remote, Rt};
}

pub use {
    mm1_address as address, mm1_common as common, mm1_core as core, mm1_node as node,
    mm1_sup as sup,
};
