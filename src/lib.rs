//! guitar-fx core: the effects, the live chain with its control channel, and device I/O.
//! The command-line tool and the UI are both thin front ends over this crate.

pub mod chain;
pub mod effects;
pub mod listen;
pub mod live;
pub mod nam;
pub mod params;
pub mod paths;
pub mod preset;
pub mod tools;
