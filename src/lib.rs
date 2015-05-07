//! Bindings for [netfilter_queue](http://netfilter.org/projects/libnetfilter_queue/doxygen/index.html)
//!
//! These bindings allow you to have access to the `QUEUE` and `NFQUEUE`, set in `iptables`,
//! and write your own userspace programs to process these queues.
#![deny(missing_docs)]

extern crate libc;
extern crate num;
#[macro_use]
extern crate lazy_static;

mod ffi;

mod util;
mod lock;

pub mod error;
pub mod handle;
pub mod queue;
pub mod message;

//#[cfg(test)]
//mod test;
