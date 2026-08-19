//! PROTOTYPE — throwaway skeleton of the core crate (wayfinder ticket #54).
//!
//! Exists to *feel* the shape decided on the map (ADR-0001..0006): domain
//! types, a sync `Model::handle(msg, now) -> Vec<Effect>`, one async-stream
//! port with a hand-rolled fake, and an Effect Runner generic over a `Ports`
//! bundle. Not production; discard or keep as reference.

pub mod domain;
pub mod model;
pub mod ports;
pub mod runner;

#[cfg(any(test, feature = "fakes"))]
pub mod testing;
