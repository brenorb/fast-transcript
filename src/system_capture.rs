//! A private, unmuted Core Audio tap exposed as a CPAL input device.
//! Requires macOS 14.2, like the transport's tap imports. Nothing here starts
//! capture merely by loading the CLI.

use anyhow::{bail, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait};
use objc2::rc::autoreleasepool;
use objc2_core_audio::*;
use objc2_core_foundation::{
    CFArray, CFBoolean, CFDictionary, CFNumber, CFRetained, CFString, CFType,
};
use objc2_foundation::{NSArray, NSNumber, NSString, NSUUID};
use std::ffi::CStr;
use std::mem::{size_of, MaybeUninit};
use std::ptr::NonNull;
use std::time::Duration;

type Destroy = unsafe extern "C-unwind" fn(AudioObjectID) -> i32;

struct Resources {
    tap: AudioObjectID,
    aggregate: AudioObjectID,
    destroy_tap: Destroy,
    destroy_aggregate: Destroy,
}

impl Resources {
    fn stop(&mut self) -> Result<()> {
        let mut failures = Vec::new();
        // The caller must drop its input stream before destroying its device.
        for (name, id, destroy) in [
            (
                "aggregate device",
                &mut self.aggregate,
                self.destroy_aggregate,
            ),
            ("process tap", &mut self.tap, self.destroy_tap),
        ] {
            if *id == kAudioObjectUnknown {
                continue;
            }
            // IDs are owned by this guard and each successful destruction clears
            // its ID. Failed cleanup retains ownership for a final Drop retry.
            let status = unsafe { destroy(*id) };
            if status == 0 {
                *id = kAudioObjectUnknown;
            } else {
                failures.push(format!("{name}: OSStatus {status}"));
            }
        }
        if !failures.is_empty() {
            bail!(
                "cannot release system audio capture ({})",
                failures.join(", ")
            );
        }
        Ok(())
    }
}

impl Drop for Resources {
    fn drop(&mut self) {
        if let Err(error) = self.stop() {
            eprintln!("warning: {error:#}");
        }
    }
}

pub(crate) struct SystemTap {
    resources: Resources,
}

fn check(status: i32, operation: &str) -> Result<()> {
    if status != 0 {
        bail!("cannot {operation}: Core Audio OSStatus {status}");
    }
    Ok(())
}

/// The three callers below pair each selector with its SDK-documented output
/// type: AudioObjectID or an owned CFString pointer, and optional pid_t input.
unsafe fn property<T>(object: AudioObjectID, selector: u32, pid: Option<&i32>) -> Result<T> {
    let mut address = AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain,
    };
    let mut value = MaybeUninit::<T>::uninit();
    let mut size = size_of::<T>() as u32;
    let (qualifier_size, qualifier) = pid
        .map(|pid| (size_of::<i32>() as u32, (pid as *const i32).cast()))
        .unwrap_or((0, std::ptr::null()));
    let status = unsafe {
        AudioObjectGetPropertyData(
            object,
            NonNull::from(&mut address),
            qualifier_size,
            qualifier,
            NonNull::from(&mut size),
            NonNull::from(&mut value).cast(),
        )
    };
    check(status, "inspect the system audio device")?;
    if size != size_of::<T>() as u32 {
        bail!("unexpected Core Audio property size");
    }
    Ok(unsafe { value.assume_init() })
}

fn dictionary(entries: &[(&CStr, &CFType)]) -> Result<CFRetained<CFDictionary<CFType, CFType>>> {
    let keys: Vec<_> = entries
        .iter()
        .map(|(key, _)| key.to_str().map(CFString::from_str))
        .collect::<std::result::Result<_, _>>()?;
    let key_refs: Vec<&CFType> = keys.iter().map(|key| key.as_ref()).collect();
    let values: Vec<_> = entries.iter().map(|(_, value)| *value).collect();
    Ok(CFDictionary::from_slices(&key_refs, &values))
}

impl SystemTap {
    /// Only an explicit meeting start calls this. Creation/start can trigger the
    /// OS system-audio permission flow; never use it as a passive preflight.
    pub(crate) fn create() -> Result<(Self, cpal::Device)> {
        let (resources, uid) = autoreleasepool(|_| -> Result<_> {
            let mut resources = Resources {
                tap: kAudioObjectUnknown,
                aggregate: kAudioObjectUnknown,
                destroy_tap: AudioHardwareDestroyProcessTap,
                destroy_aggregate: AudioHardwareDestroyAggregateDevice,
            };
            let output: AudioObjectID = unsafe {
                property(
                    kAudioObjectSystemObject as AudioObjectID,
                    kAudioHardwarePropertyDefaultOutputDevice,
                    None,
                )?
            };
            if output == kAudioObjectUnknown {
                bail!("no system output device is available; select an output in System Settings > Sound");
            }
            let raw_uid: *mut CFString =
                unsafe { property(output, kAudioDevicePropertyDeviceUID, None)? };
            let output_uid = unsafe {
                CFRetained::from_raw(
                    NonNull::new(raw_uid).context("system output has no device UID")?,
                )
            };
            let pid = std::process::id() as i32;
            let process = unsafe {
                property::<AudioObjectID>(
                    kAudioObjectSystemObject as AudioObjectID,
                    kAudioHardwarePropertyTranslatePIDToProcessObject,
                    Some(&pid),
                )
                .ok()
                .filter(|id| *id != kAudioObjectUnknown)
            };
            // HAL may not publish our process object yet. This capture process
            // does not play audio; exclude it whenever an object is available.
            let excluded: Vec<_> = process.into_iter().map(NSNumber::new_u32).collect();
            let uuid = NSUUID::new();
            let tap_uid = CFString::from_str(&uuid.UUIDString().to_string());
            let description = unsafe {
                let description = CATapDescription::new();
                description.setName(&NSString::from_str("Fast Transcript System Audio"));
                description.setUUID(&uuid);
                description.setProcesses(&NSArray::from_retained_slice(&excluded));
                description.setPrivate(true);
                description.setMuteBehavior(CATapMuteBehavior::Unmuted);
                description.setMixdown(true);
                description.setMono(true);
                description.setExclusive(true);
                description.setDeviceUID(Some(&NSString::from_str(&output_uid.to_string())));
                // Required for device-bound taps on Sonoma; omission can return
                // noErr with tap ID 0 (OpenOats' documented compatibility fix).
                description.setStream(Some(&NSNumber::new_u32(0)));
                description
            };
            let status =
                unsafe { AudioHardwareCreateProcessTap(Some(&description), &mut resources.tap) };
            check(status, "create the system audio tap")?;
            if resources.tap == kAudioObjectUnknown {
                bail!("Core Audio did not create a usable system audio tap");
            }
            let sub_tap = dictionary(&[
                (kAudioSubTapUIDKey, tap_uid.as_ref()),
                (
                    kAudioSubTapDriftCompensationKey,
                    CFBoolean::new(true).as_ref(),
                ),
            ])?;
            let taps: CFRetained<CFArray<CFType>> = CFArray::from_objects(&[sub_tap.as_ref()]);
            let uid = NSUUID::new().UUIDString().to_string();
            let aggregate_uid = CFString::from_str(&uid);
            let name = CFString::from_str("Fast Transcript System Audio");
            let aggregate = dictionary(&[
                (kAudioAggregateDeviceNameKey, name.as_ref()),
                (kAudioAggregateDeviceUIDKey, aggregate_uid.as_ref()),
                (
                    kAudioAggregateDeviceIsPrivateKey,
                    CFBoolean::new(true).as_ref(),
                ),
                (
                    kAudioAggregateDeviceTapAutoStartKey,
                    CFNumber::new_i32(0).as_ref(),
                ),
                (kAudioAggregateDeviceTapListKey, taps.as_ref()),
            ])?;
            check(
                unsafe {
                    AudioHardwareCreateAggregateDevice(
                        aggregate.as_ref(),
                        NonNull::from(&mut resources.aggregate),
                    )
                },
                "create the system audio input",
            )?;
            if resources.aggregate == kAudioObjectUnknown {
                bail!("Core Audio did not create a usable system audio input");
            }
            Ok((resources, uid))
        })?;
        let tap = Self { resources };
        // Newly connected devices can need time to publish their input format.
        // Match the fresh UID exactly; never fall back to a different input.
        for _ in 0..40 {
            if let Some(device) = cpal::default_host().devices()?.find(|device| {
                device.id().is_ok_and(|id| id.id() == uid)
                    && device.default_input_config().is_ok()
                    && device
                        .supported_input_configs()
                        .is_ok_and(|mut formats| formats.next().is_some())
            }) {
                return Ok((tap, device));
            }
            std::thread::sleep(Duration::from_millis(75));
        }
        bail!("the private system audio device did not become available");
    }

    /// Drop the corresponding CPAL stream before calling this.
    pub(crate) fn stop(&mut self) -> Result<()> {
        self.resources.stop()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Calls {
        events: Vec<(&'static str, u32)>,
        fail_once: [bool; 2],
    }
    thread_local! { static CALLS: RefCell<Calls> = RefCell::default(); }

    fn destroy(kind: &'static str, slot: usize, id: u32) -> i32 {
        CALLS.with(|state| {
            let mut state = state.borrow_mut();
            state.events.push((kind, id));
            if std::mem::take(&mut state.fail_once[slot]) {
                -1
            } else {
                0
            }
        })
    }
    unsafe extern "C-unwind" fn aggregate(id: u32) -> i32 {
        destroy("aggregate", 0, id)
    }
    unsafe extern "C-unwind" fn tap(id: u32) -> i32 {
        destroy("tap", 1, id)
    }

    #[test]
    fn partial_initialization_and_failed_cleanup_release_each_owned_resource() {
        for initialized in 0..4 {
            for failures in 0..4 {
                CALLS.with(|state| {
                    *state.borrow_mut() = Calls {
                        events: Vec::new(),
                        fail_once: [failures & 1 != 0, failures & 2 != 0],
                    }
                });
                let mut resources = Resources {
                    tap: if initialized & 1 != 0 { 12 } else { 0 },
                    aggregate: if initialized & 2 != 0 { 34 } else { 0 },
                    destroy_tap: tap,
                    destroy_aggregate: aggregate,
                };
                let failed = resources.stop().is_err();
                assert_eq!(
                    failed,
                    (initialized & 2 != 0 && failures & 1 != 0)
                        || (initialized & 1 != 0 && failures & 2 != 0)
                );
                if !failed {
                    resources.stop().unwrap();
                }
                drop(resources);
                CALLS.with(|state| {
                    let events = &state.borrow().events;
                    assert!(events.iter().all(|(_, id)| *id != 0));
                    for (name, present, failure) in [
                        ("aggregate", initialized & 2 != 0, failures & 1 != 0),
                        ("tap", initialized & 1 != 0, failures & 2 != 0),
                    ] {
                        assert_eq!(
                            events.iter().filter(|(kind, _)| *kind == name).count(),
                            if present {
                                if failure {
                                    2
                                } else {
                                    1
                                }
                            } else {
                                0
                            }
                        );
                    }
                    if initialized == 3 {
                        assert_eq!(events[0], ("aggregate", 34));
                    }
                });
            }
        }
    }
}
