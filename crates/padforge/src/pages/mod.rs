//! Page implementations.

pub mod controller;
pub mod dashboard;
pub mod gyro;
pub mod mapping;
pub mod output;
pub mod profiles;
pub mod settings;

use padcore::engine::EngineHandle;

use crate::state::AppState;

/// Shared context handed to every page.
pub struct Ctx<'a> {
    pub state: &'a mut AppState,
    pub engine: &'a EngineHandle,
    /// Raw key events for this frame, needed by the hotkey capture UI.
    pub input_keys: Vec<egui::Event>,
}