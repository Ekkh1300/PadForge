//! One app per pad.
//!
//! PadForge lives behind a tray icon, and that is what makes a second copy
//! both easy to start and easy to miss: a shortcut double-click while the
//! window hides in the notification area looks like "nothing happened", and
//! what really happens is that a second engine starts fighting the first for
//! one DualShock's reports while two icons sit in the tray — and a pair of
//! engines reading the same pad is exactly the kind of contention that shows
//! up hours later as input that stutters everywhere.
//!
//! So a second launch never becomes a second instance: a named mutex answers
//! "somebody is already here", a named event carries the wake-up, and the
//! newcomer leaves. The first instance polls the event on every frame it is
//! awake — a wait with zero timeout, which is one syscall — and pulls its
//! window back onto the screen. Relaunching the app therefore ends with the
//! window visible, which is what the person holding the mouse was after.
//!
//! Both names use the `Local\` prefix, so the copies are scoped to the login
//! session and a second user session can run its own instance.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, WAIT_OBJECT_0,
};
use windows_sys::Win32::System::Threading::{
    CreateEventW, CreateMutexW, SetEvent, WaitForSingleObject,
};

/// Who holds the right to run.
///
/// `GuardUnavailable` is not a refusal: if the mutex could not be created the
/// app still starts, because refusing to run would brick it over a kernel
/// object that should never fail. The guard is best effort; the wake-up is
/// what needs its own note when it is missing.
pub enum FirstInstance {
    /// Running and holding the lock.
    Yes(InstanceGuard),
    /// Another copy exists; it has been told to show itself.
    NotifiedExisting,
    /// The lock could not be created; running without one.
    Unavailable(String),
}

/// The mutex name — no second instance shares the pad.
const MUTEX_NAME: &str = "Local\\PadForge.instance";

/// The wake event — "please show yourself", set by a refused second copy.
const WAKE_NAME: &str = "Local\\PadForge.show";

/// Holds the single-instance right and listens for wake-ups, for the life of
/// the process.
pub struct InstanceGuard {
    /// The mutex handle. The OS releases the named mutex when this — or the
    /// process — goes away, so no explicit release is needed on exit.
    mutex: windows_sys::Win32::Foundation::HANDLE,
    /// Auto-reset wake event. A wait with zero timeout is both the poll and
    /// the clear: once set, the next wait consumes the signal.
    wake: Option<windows_sys::Win32::Foundation::HANDLE>,
}

// OS handles are plain integer-ish ids with no thread affinity, and this one
// lives on the main thread through the app that owns it.
unsafe impl Send for InstanceGuard {}
unsafe impl Sync for InstanceGuard {}

/// Debug without dumping raw handle values, which mean nothing and would
/// differ run to run.
impl std::fmt::Debug for FirstInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FirstInstance::Yes(_) => f.write_str("Yes(InstanceGuard)"),
            FirstInstance::NotifiedExisting => f.write_str("NotifiedExisting"),
            FirstInstance::Unavailable(reason) => {
                write!(f, "Unavailable({reason})")
            }
        }
    }
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.mutex);
            if let Some(wake) = self.wake {
                CloseHandle(wake);
            }
        }
    }
}

impl InstanceGuard {
    /// Whether a second copy asked this one to show itself.
    ///
    /// One poll per frame: a zero-timeout wait is a couple of microseconds
    /// when nothing is pending, and it clears the event when something is.
    pub fn show_requested(&self) -> bool {
        match self.wake {
            Some(wake) => unsafe { WaitForSingleObject(wake, 0) == WAIT_OBJECT_0 },
            None => false,
        }
    }
}

/// Who claims the right to run, with the kernel names made explicit so the
/// tests can use their own names instead of racing the real app for them.
pub fn acquire_named(mutex_name: &str, wake_name: &str) -> FirstInstance {
    unsafe {
        // No owner at creation: the mutex is the flag itself, and every copy
        // that finds it existing leaves.
        let mutex_name = wide(mutex_name);
        let mutex = CreateMutexW(std::ptr::null(), 0, mutex_name.as_ptr());
        if mutex.is_null() {
            return FirstInstance::Unavailable(format!(
                "CreateMutexW failed with code {}",
                GetLastError()
            ));
        }

        if GetLastError() == ERROR_ALREADY_EXISTS {
            // The winner's wake event also exists, so this opens it and sets
            // it. If anything here fails the signal is lost, which leaves a
            // second copy that silently leaves — the old behaviour, not a
            // worse one.
            let wake_name = wide(wake_name);
            let wake = CreateEventW(std::ptr::null(), 0, 0, wake_name.as_ptr());
            if !wake.is_null() {
                SetEvent(wake);
                CloseHandle(wake);
            }
            CloseHandle(mutex);
            return FirstInstance::NotifiedExisting;
        }

        // An auto-reset event, unset: `show_requested` then consumes each
        // wake-up exactly once. Losing this handle would mean wake-ups from
        // later copies could not reach this one, so the run continues without
        // the wake path rather than without the instance guard.
        let wake_name = wide(wake_name);
        let wake = CreateEventW(std::ptr::null(), 0, 0, wake_name.as_ptr());
        let wake = (!wake.is_null()).then_some(wake);
        FirstInstance::Yes(InstanceGuard { mutex, wake })
    }
}

/// The real names for the real app.
pub fn acquire() -> FirstInstance {
    acquire_named(MUTEX_NAME, WAKE_NAME)
}

/// UTF-16 with a terminator, which every wide-string Win32 call here wants.
fn wide(text: &str) -> Vec<u16> {
    OsStr::new(text).encode_wide().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Inside one process, a second `acquire` behaves exactly as a second
    /// launch would: it sees the first, raises the wake-up, and leaves — and
    /// the first's zero-timeout wait consumes the signal once.
    ///
    /// The names carry the process id, so the test never races the real app
    /// (which holds `Local\PadForge.instance` while it runs) nor a parallel
    /// run of the test binary itself.
    #[test]
    fn a_second_acquire_notifies_the_first_and_leaves() {
        let tag = std::process::id();
        let mutex = format!("Local\\PadForge.test-{tag}.instance");
        let wake = format!("Local\\PadForge.test-{tag}.wake");

        let first = acquire_named(&mutex, &wake);
        let guard = match &first {
            FirstInstance::Yes(guard) => guard,
            other => panic!("expected to become the first instance, got {other:?}"),
        };
        assert!(
            !guard.show_requested(),
            "an instance that was never woken must not see a wake-up"
        );

        // The second must not answer "yes, you are first" — otherwise two
        // copies would legitimately run and fight over the pad.
        let second = acquire_named(&mutex, &wake);
        assert!(
            matches!(second, FirstInstance::NotifiedExisting),
            "a second acquire must be refused, got {second:?}"
        );

        assert!(
            guard.show_requested(),
            "the refusal must wake the first instance"
        );
        assert!(
            !guard.show_requested(),
            "an auto-reset wake-up must be consumed by one poll, not repeat"
        );

        // Tidy the kernel objects before the next run's names are reused.
        drop(first);
    }
}
