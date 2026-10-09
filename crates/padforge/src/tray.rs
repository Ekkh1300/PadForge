//! System tray integration.
//!
//! The tray is what makes "close to the background" work: the window can hide
//! entirely and the app keeps running, reachable from the notification area.
//!
//! `tray-icon` publishes events through global receivers rather than a per-icon
//! channel, so the pump polls them with `try_recv`. That is cheap: the channels
//! are almost always empty, and the loop sleeps when there is nothing to do.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIcon as RawTrayIcon, TrayIconBuilder, TrayIconEvent};

use padcore::engine::{EngineCommand, EngineHandle};

/// What the tray can ask the UI to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayCommand {
    /// Show and focus the window.
    Show,
    /// Pause or resume output.
    Pause,
    NextProfile,
    Recalibrate,
    /// Quit the app.
    Quit,
}

/// Commands raised by the tray, drained by the UI each frame.
pub type TrayInbox = Arc<Mutex<Vec<TrayCommand>>>;

/// Menu item ids, paired with the commands they map to.
const ID_SHOW: &str = "show";
const ID_PAUSE: &str = "pause";
const ID_NEXT: &str = "next_profile";
const ID_RECAL: &str = "recalibrate";
const ID_QUIT: &str = "quit";

/// How often the pump checks for events when it has nothing to do.
const IDLE_POLL: Duration = Duration::from_millis(80);

/// Owns the tray icon. Dropping it removes the icon from the notification area.
pub struct TrayIcon {
    _icon: RawTrayIcon,
}

impl TrayIcon {
    /// Build the tray icon with its context menu.
    pub fn new(outbox: TrayInbox) -> Result<Self, String> {
        let menu = Menu::new();
        let items = [
            (ID_SHOW, "Open PadForge"),
            (ID_PAUSE, "Pause / resume output"),
            (ID_NEXT, "Next profile"),
            (ID_RECAL, "Recalibrate sticks"),
            (ID_QUIT, "Quit PadForge"),
        ];
        for (_id, label) in items {
            let item = MenuItem::new(label, true, None);
            menu.append(&item).map_err(|e| format!("{e:?}"))?;
        }
        menu.append(&PredefinedMenuItem::separator())
            .map_err(|e| format!("{e:?}"))?;

        // The icon is generated rather than shipped, so validate it first: a
        // bad icon would otherwise surface as an opaque build error.
        let rgba = crate::icons::tray_icon_rgba(32);
        let icon = tray_icon::Icon::from_rgba(rgba, 32, 32).map_err(|e| format!("{e:?}"))?;

        let built = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("PadForge - DualShock 4 to Windows")
            .with_icon(icon)
            .build()
            .map_err(|e| format!("{e:?}"))?;

        // Pump on a background thread so the UI frame is never blocked.
        std::thread::Builder::new()
            .name("padforge-tray".into())
            .spawn(move || pump(&outbox))
            .map_err(|e| format!("could not start the tray thread: {e}"))?;

        Ok(Self { _icon: built })
    }
}

/// Drain tray and menu events into the inbox.
fn pump(outbox: &TrayInbox) {
    use tray_icon::TrayIconEvent::{Click, DoubleClick};

    loop {
        let mut handled = false;

        // Left click, or a double click, shows the window.
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            handled = true;
            use tray_icon::MouseButton::{Left, Right};
            use tray_icon::MouseButtonState::{Down, Up};
            match event {
                Click {
                    button: Left,
                    button_state: Up,
                    ..
                }
                | DoubleClick {
                    button: Left,
                    ..
                } => push(outbox, TrayCommand::Show),
                Click {
                    button: Right,
                    button_state: Down,
                    ..
                } => {
                    // The OS shows the menu; nothing to do here.
                }
                _ => {}
            }
        }

        // Menu selections.
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            handled = true;
            match event.id.as_ref() {
                ID_SHOW => push(outbox, TrayCommand::Show),
                ID_PAUSE => push(outbox, TrayCommand::Pause),
                ID_NEXT => push(outbox, TrayCommand::NextProfile),
                ID_RECAL => push(outbox, TrayCommand::Recalibrate),
                ID_QUIT => {
                    push(outbox, TrayCommand::Quit);
                    return;
                }
                _ => {}
            }
        }

        if !handled {
            std::thread::sleep(IDLE_POLL);
        }
    }
}

fn push(outbox: &TrayInbox, cmd: TrayCommand) {
    if let Ok(mut v) = outbox.lock() {
        v.push(cmd);
    }
}

/// Route a tray command. Returns true when the engine handled it directly, false
/// when the UI needs to act (show or quit).
pub fn apply(cmd: TrayCommand, engine: &EngineHandle) -> bool {
    match cmd {
        TrayCommand::Show | TrayCommand::Quit => false,
        TrayCommand::Pause => {
            engine.send(EngineCommand::TogglePause);
            true
        }
        TrayCommand::NextProfile => {
            engine.send(EngineCommand::CycleProfile(1));
            true
        }
        TrayCommand::Recalibrate => {
            engine.send(EngineCommand::Recalibrate);
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inbox_collects_commands() {
        let inbox: TrayInbox = Arc::new(Mutex::new(Vec::new()));
        push(&inbox, TrayCommand::Show);
        push(&inbox, TrayCommand::Quit);
        assert_eq!(inbox.lock().unwrap().len(), 2);
    }

    #[test]
    fn menu_ids_are_distinct() {
        let ids = [ID_SHOW, ID_PAUSE, ID_NEXT, ID_RECAL, ID_QUIT];
        let mut sorted = ids.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len());
    }

    #[test]
    fn tray_icon_pixels_are_valid_for_the_builder() {
        // A mismatched pixel count is the most likely way `Icon::from_rgba`
        // fails, so check it here rather than at runtime.
        let rgba = crate::icons::tray_icon_rgba(32);
        assert_eq!(rgba.len(), (32 * 32 * 4) as usize);
        assert_eq!(rgba.len() % 4, 0);
        assert!(tray_icon::Icon::from_rgba(rgba, 32, 32).is_ok());
    }
}