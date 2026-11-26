//! macOS global hold gesture for both terminal and desktop recording clients.

use anyhow::Result;

#[cfg(any(target_os = "macos", test))]
#[derive(Default)]
struct HoldState {
    held: bool,
    overflow: bool,
    events: std::collections::VecDeque<bool>,
}
#[cfg(any(target_os = "macos", test))]
impl HoldState {
    fn push(&mut self, held: bool) {
        if self.held == held {
            return;
        }
        self.held = held;
        if self.events.len() < 64 {
            self.events.push_back(held);
        } else {
            self.overflow = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn property_hold_edges_ignore_repeats_and_cannot_silently_lose_release() {
        let mut random = 91u64;
        for _ in 0..10_000 {
            let mut state = HoldState::default();
            for _ in 0..64 {
                random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                let held = random >> 63 != 0;
                state.push(held);
                assert_eq!(state.held, held);
            }
            assert!(!state.overflow);
            let events: Vec<_> = state.events.iter().copied().collect();
            assert!(events.windows(2).all(|pair| pair[0] != pair[1]));
            if !events.is_empty() {
                assert!(events[0]);
                assert_eq!(events.last(), Some(&state.held));
            }
        }
        let mut state = HoldState::default();
        for index in 0..100 {
            state.push(index % 2 == 0);
        }
        assert!(state.overflow);
        assert_eq!(state.events.len(), 64);
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) struct Shortcut;
#[cfg(not(target_os = "macos"))]
impl Shortcut {
    pub(crate) fn register(_choice: &str) -> Result<Self> {
        anyhow::bail!("global dictation shortcut currently requires macOS")
    }
    pub(crate) fn label(&self) -> &str {
        ""
    }
    pub(crate) fn poll(&mut self) -> Result<Vec<bool>> {
        Ok(Vec::new())
    }
}

#[cfg(target_os = "macos")]
pub(crate) use macos::Shortcut;

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use anyhow::{bail, Context};
    use std::ffi::c_void;
    type Ref = *mut c_void;
    type Status = i32;
    #[repr(C)]
    #[derive(Default)]
    struct KeyID {
        signature: u32,
        id: u32,
    }
    #[repr(C)]
    struct EventType {
        class: u32,
        kind: u32,
    }
    #[link(name = "Carbon", kind = "framework")]
    extern "C" {
        fn GetApplicationEventTarget() -> Ref;
        fn GetEventDispatcherTarget() -> Ref;
        fn InstallEventHandler(
            target: Ref,
            handler: unsafe extern "C" fn(Ref, Ref, Ref) -> Status,
            count: usize,
            types: *const EventType,
            context: Ref,
            result: *mut Ref,
        ) -> Status;
        fn RemoveEventHandler(handler: Ref) -> Status;
        fn RegisterEventHotKey(
            key: u32,
            modifiers: u32,
            id: KeyID,
            target: Ref,
            options: u32,
            result: *mut Ref,
        ) -> Status;
        fn UnregisterEventHotKey(key: Ref) -> Status;
        fn ReceiveNextEvent(
            count: usize,
            types: *const EventType,
            timeout: f64,
            pull: bool,
            event: *mut Ref,
        ) -> Status;
        fn SendEventToEventTarget(event: Ref, target: Ref) -> Status;
        fn ReleaseEvent(event: Ref);
        fn GetEventKind(event: Ref) -> u32;
        fn GetEventParameter(
            event: Ref,
            name: u32,
            kind: u32,
            actual_type: *mut u32,
            size: usize,
            actual_size: *mut usize,
            data: Ref,
        ) -> Status;
    }
    const SIGNATURE: u32 = u32::from_be_bytes(*b"fscp");
    unsafe extern "C" fn event(_: Ref, event: Ref, context: Ref) -> Status {
        let mut id = KeyID::default();
        if GetEventParameter(
            event,
            u32::from_be_bytes(*b"----"),
            u32::from_be_bytes(*b"hkid"),
            std::ptr::null_mut(),
            std::mem::size_of::<KeyID>(),
            std::ptr::null_mut(),
            &mut id as *mut _ as Ref,
        ) != 0
            || id.signature != SIGNATURE
            || id.id != 1
        {
            return -9874;
        }
        let state = &mut *(context as *mut HoldState);
        let held = GetEventKind(event) == 5;
        state.push(held);
        0
    }
    pub(crate) struct Shortcut {
        key: Ref,
        handler: Ref,
        state: Box<HoldState>,
        label: &'static str,
    }
    impl Shortcut {
        pub(crate) fn register(choice: &str) -> Result<Self> {
            let mut this = Self {
                key: std::ptr::null_mut(),
                handler: std::ptr::null_mut(),
                label: "",
                state: Box::default(),
            };
            let types = [
                EventType {
                    class: u32::from_be_bytes(*b"keyb"),
                    kind: 5,
                },
                EventType {
                    class: u32::from_be_bytes(*b"keyb"),
                    kind: 6,
                },
            ];
            unsafe {
                let status = InstallEventHandler(
                    GetApplicationEventTarget(),
                    event,
                    2,
                    types.as_ptr(),
                    &mut *this.state as *mut _ as Ref,
                    &mut this.handler,
                );
                if status != 0 {
                    bail!("cannot install dictation shortcut handler ({status})");
                }
                let candidates = [
                    ("ctrl+shift+space", 49, "Control-Shift-Space"),
                    ("ctrl+shift+d", 2, "Control-Shift-D"),
                    ("ctrl+shift+period", 47, "Control-Shift-Period"),
                ];
                let mut last_status = 0;
                for (name, key, label) in candidates {
                    if choice != "auto" && choice != name {
                        continue;
                    }
                    // Apple requires exclusive registration to report collisions.
                    // Without it, registration may succeed but never fire.
                    let mut registered = std::ptr::null_mut();
                    let status = RegisterEventHotKey(
                        key,
                        (1 << 12) | (1 << 9),
                        KeyID {
                            signature: SIGNATURE,
                            id: 1,
                        },
                        GetApplicationEventTarget(),
                        1,
                        &mut registered,
                    );
                    if status == 0 {
                        this.key = registered;
                        this.label = label;
                        break;
                    }
                    last_status = status;
                }
                if this.key.is_null() {
                    bail!("dictation shortcut {choice} is unavailable ({last_status}); choose another --hotkey chord or use the hold button");
                }
            }
            Ok(this)
        }
        pub(crate) fn label(&self) -> &str {
            self.label
        }
        pub(crate) fn poll(&mut self) -> Result<Vec<bool>> {
            unsafe {
                for _ in 0..128 {
                    let mut event = std::ptr::null_mut();
                    if ReceiveNextEvent(0, std::ptr::null(), 0.001, true, &mut event) != 0 {
                        break;
                    }
                    if event.is_null() {
                        break;
                    }
                    SendEventToEventTarget(event, GetEventDispatcherTarget());
                    ReleaseEvent(event);
                }
            }
            if self.state.overflow {
                return Err(anyhow::anyhow!("dictation shortcut event queue overflowed"))
                    .context("recording stopped to avoid losing the release gesture");
            }
            Ok(self.state.events.drain(..).collect())
        }
    }
    impl Drop for Shortcut {
        fn drop(&mut self) {
            unsafe {
                if !self.key.is_null() {
                    UnregisterEventHotKey(self.key);
                }
                if !self.handler.is_null() {
                    RemoveEventHandler(self.handler);
                }
            }
        }
    }
}
