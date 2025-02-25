//! This module contains general types used by the embassy-rs Vendor Example.

// Copyright (c) 2025 Piers Finlayson <piers@piers.rocks>
//
// MIT licensed - see https://opensource.org/licenses/MIT

/// Direction - used to apply to Control transfers, and other operations/
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// In is from the device to the host.
    In,

    /// Out is from the host to the device.
    Out,
}

// Implement Format so Direction can be formatted by defmt.
impl defmt::Format for Direction {
    fn format(&self, f: defmt::Formatter) {
        match self {
            Direction::In => defmt::write!(f, "In"),
            Direction::Out => defmt::write!(f, "Out"),
        }
    }
}
