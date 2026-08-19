//! Dependency-light control policy and parameter surface for the rig.

#![no_std]

#[cfg(test)]
extern crate std;

pub mod control;
pub mod control_params;
pub mod safety_limits;
