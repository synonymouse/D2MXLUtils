//! Automap markers: room scanning and the single marker-chain owner.

#![cfg(any(target_os = "windows", target_os = "linux"))]

mod manager;
mod scanner;

pub(crate) use scanner::MarkerScanner;

#[cfg(all(test, target_os = "windows"))]
pub(crate) use manager::test_support::native as test_support;
