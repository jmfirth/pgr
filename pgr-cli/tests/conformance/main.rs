#![warn(clippy::pedantic)]
#![allow(clippy::missing_panics_doc)] // Conformance tests use unwrap/expect freely
#![allow(dead_code)] // Shared harness/helpers used by conformance suites from Tasks 126-129

mod compare;
mod display;
mod harness;
mod helpers;
mod navigation;
mod search;
