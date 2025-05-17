pub trait Now: Send {
    type Instant;
    fn now(&self) -> Self::Instant;
}
