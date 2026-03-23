#![warn(clippy::pedantic)]
#![allow(clippy::missing_panics_doc)]
// Conformance tests use unwrap/expect freely
// Test infrastructure modules define utilities for current and future test tasks.
#![allow(dead_code)]

mod compare;
mod harness;
mod helpers;
mod navigation;
