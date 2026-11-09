#![forbid(unsafe_code)]
#![allow(dead_code, unused_imports)]
//! `upwell-review`: an independent, read-only, two-stage pull request review for `upwell-rs/upwell`.

mod assets;
mod diffmap;
mod drafts;
mod git;
mod github;
mod guard;
mod lock;
mod model;
mod post;
mod provider;
mod schema;
mod stream;
#[cfg(test)]
mod testing;
mod tools;
mod verify;

fn main() {}
