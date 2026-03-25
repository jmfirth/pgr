#![warn(clippy::pedantic)]
#![allow(clippy::missing_panics_doc)] // Conformance tests use unwrap/expect freely
#![allow(dead_code)] // Shared harness/helpers used by conformance suites from Tasks 126-129

mod compare;
mod display;
mod file_misc;
mod harness;
mod helpers;
mod navigation;
mod phase2_keys;
mod phase2_multifile;
mod phase2_navigation;
mod phase2_options;
mod phase2_prompt;
mod phase2_search;
mod search;
