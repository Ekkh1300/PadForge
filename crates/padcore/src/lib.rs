//! PadForge core engine.
//!
//! Turns a physical PlayStation DualShock 4 into a first-class Windows input
//! device: it reads the raw HID report, reshapes every axis through a filter
//! chain, remaps each control onto whatever target you like, and publishes the
//! result as a virtual gamepad.

pub mod device;
pub mod engine;
pub mod filters;
pub mod gyro;
pub mod hotkey;
pub mod mapping;
pub mod output;
pub mod paths;
pub mod process;
pub mod pointer;
pub mod profile;
pub mod report;
pub mod settings;
pub mod touchpad;

pub use engine::{spawn, EngineCommand, EngineEvent, EngineHandle, EventLevel, Telemetry};
pub use mapping::{Ds4Control, Mapping, X360Control};
pub use pointer::{
    GyroPointer, MouseButton, PointerConfig, PointerInvert, TouchpadPointer,
};
pub use profile::Profile;
pub use settings::Settings;

pub const APP_NAME: &str = "PadForge";
pub const APP_SLUG: &str = "padforge";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");