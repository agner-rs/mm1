use std::sync::Arc;

use crate::message::Message;

impl<T> Message for Option<T>
where
    Self: Send + 'static,
    T: Message,
{
}
impl<T, E> Message for Result<T, E>
where
    Self: Send + 'static,
    T: Message,
    E: Message,
{
}
impl<T> Message for Arc<T> where T: Message + Sync {}
impl<T> Message for Vec<T> where T: Message {}

macro_rules! impl_message_for_primitives {
    ( $( $t:ty ),+ $(,)? ) => {
        $(
            impl Message for $t {}
        )+
    };
}

impl_message_for_primitives! {
    u8, i8, u16, i16, u32, i32, u64, i64, u128, i128, usize, isize,
    f32, f64,
    bool, char,
    (),
    String,
    std::time::Duration,
    &'static str,
}

macro_rules! impl_message_for_tuples {
    ($( $len:literal => ( $( $T:ident ),+ ) ),+ $(,)?) => {
        $(
            impl<$( $T ),+> Message for ( $( $T, )+ )
            where
                $( $T: Message ),+
            {}
        )+
    };
}

impl_message_for_tuples! {
    1  => (T0),
    2  => (T0, T1),
    3  => (T0, T1, T2),
    4  => (T0, T1, T2, T3),
    5  => (T0, T1, T2, T3, T4),
    6  => (T0, T1, T2, T3, T4, T5),
    7  => (T0, T1, T2, T3, T4, T5, T6),
    8  => (T0, T1, T2, T3, T4, T5, T6, T7),
    9  => (T0, T1, T2, T3, T4, T5, T6, T7, T8),
    10 => (T0, T1, T2, T3, T4, T5, T6, T7, T8, T9),
    11 => (T0, T1, T2, T3, T4, T5, T6, T7, T8, T9, T10),
    12 => (T0, T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11),
}
