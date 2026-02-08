//! KWarpd Input Manager
//!
//! Handles keyboard input interception via evdev and EVIOCGRAB

use anyhow::{Context, Result};
use evdev::{Device, EventType, KeyCode};
use std::fs;
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;

use crate::config::{KeyBinding, Modifiers};

/// Maps evdev key codes to readable key names
fn key_to_name(key: KeyCode) -> Option<String> {
    let name = match key {
        KeyCode::KEY_A => "a",
        KeyCode::KEY_B => "b",
        KeyCode::KEY_C => "c",
        KeyCode::KEY_D => "d",
        KeyCode::KEY_E => "e",
        KeyCode::KEY_F => "f",
        KeyCode::KEY_G => "g",
        KeyCode::KEY_H => "h",
        KeyCode::KEY_I => "i",
        KeyCode::KEY_J => "j",
        KeyCode::KEY_K => "k",
        KeyCode::KEY_L => "l",
        KeyCode::KEY_M => "m",
        KeyCode::KEY_N => "n",
        KeyCode::KEY_O => "o",
        KeyCode::KEY_P => "p",
        KeyCode::KEY_Q => "q",
        KeyCode::KEY_R => "r",
        KeyCode::KEY_S => "s",
        KeyCode::KEY_T => "t",
        KeyCode::KEY_U => "u",
        KeyCode::KEY_V => "v",
        KeyCode::KEY_W => "w",
        KeyCode::KEY_X => "x",
        KeyCode::KEY_Y => "y",
        KeyCode::KEY_Z => "z",
        KeyCode::KEY_0 => "0",
        KeyCode::KEY_1 => "1",
        KeyCode::KEY_2 => "2",
        KeyCode::KEY_3 => "3",
        KeyCode::KEY_4 => "4",
        KeyCode::KEY_5 => "5",
        KeyCode::KEY_6 => "6",
        KeyCode::KEY_7 => "7",
        KeyCode::KEY_8 => "8",
        KeyCode::KEY_9 => "9",
        KeyCode::KEY_ESC => "esc",
        KeyCode::KEY_BACKSPACE => "backspace",
        KeyCode::KEY_TAB => "tab",
        KeyCode::KEY_ENTER => "enter",
        KeyCode::KEY_SPACE => "space",
        KeyCode::KEY_COMMA => ",",
        KeyCode::KEY_DOT => ".",
        KeyCode::KEY_SLASH => "/",
        KeyCode::KEY_SEMICOLON => ";",
        KeyCode::KEY_APOSTROPHE => "'",
        KeyCode::KEY_LEFTBRACE => "[",
        KeyCode::KEY_RIGHTBRACE => "]",
        KeyCode::KEY_BACKSLASH => "\\",
        KeyCode::KEY_MINUS => "-",
        KeyCode::KEY_EQUAL => "=",
        KeyCode::KEY_GRAVE => "`",
        KeyCode::KEY_UP => "up",
        KeyCode::KEY_DOWN => "down",
        KeyCode::KEY_LEFT => "left",
        KeyCode::KEY_RIGHT => "right",
        KeyCode::KEY_F1 => "f1",
        KeyCode::KEY_F2 => "f2",
        KeyCode::KEY_F3 => "f3",
        KeyCode::KEY_F4 => "f4",
        KeyCode::KEY_F5 => "f5",
        KeyCode::KEY_F6 => "f6",
        KeyCode::KEY_F7 => "f7",
        KeyCode::KEY_F8 => "f8",
        KeyCode::KEY_F9 => "f9",
        KeyCode::KEY_F10 => "f10",
        KeyCode::KEY_F11 => "f11",
        KeyCode::KEY_F12 => "f12",
        _ => return None,
    };
    Some(name.to_string())
}

/// Current modifier state
#[derive(Debug, Clone, Default)]
pub struct ModifierState {
    pub left_alt: bool,
    pub right_alt: bool,
    pub left_ctrl: bool,
    pub right_ctrl: bool,
    pub left_shift: bool,
    pub right_shift: bool,
    pub left_meta: bool,
    pub right_meta: bool,
}

impl ModifierState {
    /// Update state based on key event
    pub fn update(&mut self, key: KeyCode, pressed: bool) {
        match key {
            KeyCode::KEY_LEFTALT => self.left_alt = pressed,
            KeyCode::KEY_RIGHTALT => self.right_alt = pressed,
            KeyCode::KEY_LEFTCTRL => self.left_ctrl = pressed,
            KeyCode::KEY_RIGHTCTRL => self.right_ctrl = pressed,
            KeyCode::KEY_LEFTSHIFT => self.left_shift = pressed,
            KeyCode::KEY_RIGHTSHIFT => self.right_shift = pressed,
            KeyCode::KEY_LEFTMETA => self.left_meta = pressed,
            KeyCode::KEY_RIGHTMETA => self.right_meta = pressed,
            _ => {}
        }
    }

    /// Check if Alt is pressed
    pub fn alt(&self) -> bool {
        self.left_alt || self.right_alt
    }

    /// Check if Ctrl is pressed
    pub fn ctrl(&self) -> bool {
        self.left_ctrl || self.right_ctrl
    }

    /// Check if Shift is pressed
    pub fn shift(&self) -> bool {
        self.left_shift || self.right_shift
    }

    /// Check if Meta/Super is pressed
    pub fn meta(&self) -> bool {
        self.left_meta || self.right_meta
    }

    /// Convert to Modifiers struct for comparison
    pub fn to_modifiers(&self) -> Modifiers {
        Modifiers {
            alt: self.alt(),
            ctrl: self.ctrl(),
            shift: self.shift(),
            super_key: self.meta(),
        }
    }

    /// Check if a key binding matches current state
    pub fn matches(&self, binding: &KeyBinding, key_name: &str) -> bool {
        if key_name != binding.key {
            return false;
        }
        let mods = self.to_modifiers();
        mods == binding.modifiers
    }
}

/// A keyboard input event
#[derive(Debug, Clone)]
pub struct KeyEvent {
    pub key: String,
    pub pressed: bool,
    pub modifiers: Modifiers,
}

/// Input manager that handles keyboard device access
pub struct InputManager {
    devices: Vec<Device>,
    grabbed: bool,
    modifier_state: ModifierState,
}

impl InputManager {
    /// Create a new input manager by finding all keyboard devices
    pub fn new() -> Result<Self> {
        let devices = Self::find_keyboards()?;
        if devices.is_empty() {
            anyhow::bail!("No keyboard devices found. Do you have permission to access /dev/input?");
        }
        log::info!("Found {} keyboard device(s)", devices.len());
        Ok(Self {
            devices,
            grabbed: false,
            modifier_state: ModifierState::default(),
        })
    }

    /// Find all keyboard input devices
    fn find_keyboards() -> Result<Vec<Device>> {
        let mut keyboards = Vec::new();

        let input_dir = PathBuf::from("/dev/input");
        if !input_dir.exists() {
            anyhow::bail!("/dev/input not found");
        }

        for entry in fs::read_dir(&input_dir)? {
            let entry = entry?;
            let path = entry.path();

            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if !name.starts_with("event") {
                    continue;
                }
            }

            match Device::open(&path) {
                Ok(device) => {
                    if let Some(keys) = device.supported_keys() {
                        if keys.contains(KeyCode::KEY_A) && keys.contains(KeyCode::KEY_ENTER) {
                            log::debug!("Found keyboard: {:?} - {:?}",
                                path, device.name().unwrap_or("Unknown"));
                            keyboards.push(device);
                        }
                    }
                }
                Err(e) => {
                    log::trace!("Could not open {:?}: {}", path, e);
                }
            }
        }

        Ok(keyboards)
    }

    /// Get file descriptors for poll/select
    pub fn get_fds(&self) -> Vec<i32> {
        self.devices.iter().map(|d| d.as_raw_fd()).collect()
    }

    /// Grab all keyboard devices (exclusive access)
    pub fn grab(&mut self) -> Result<()> {
        if self.grabbed {
            return Ok(());
        }

        for device in &mut self.devices {
            device.grab()
                .with_context(|| format!("Failed to grab device: {:?}", device.name()))?;
        }
        self.grabbed = true;
        log::info!("Grabbed keyboard input");
        Ok(())
    }

    /// Release grabbed devices
    pub fn ungrab(&mut self) -> Result<()> {
        if !self.grabbed {
            return Ok(());
        }

        for device in &mut self.devices {
            if let Err(e) = device.ungrab() {
                log::warn!("Failed to ungrab device {:?}: {}", device.name(), e);
            }
        }
        self.grabbed = false;
        self.modifier_state = ModifierState::default();
        log::info!("Released keyboard input");
        Ok(())
    }

    /// Check if devices are currently grabbed
    pub fn is_grabbed(&self) -> bool {
        self.grabbed
    }

    /// Poll for events from all devices (non-blocking if possible)
    pub fn poll_events(&mut self) -> Result<Vec<KeyEvent>> {
        let mut events = Vec::new();

        for device in &mut self.devices {
            if let Ok(ev_iter) = device.fetch_events() {
                for ev in ev_iter {
                    if ev.event_type() == EventType::KEY {
                        let key = KeyCode::new(ev.code());
                        let pressed = ev.value() == 1;
                        let is_repeat = ev.value() == 2;

                        if is_repeat {
                            continue;
                        }

                        self.modifier_state.update(key, pressed);

                        if let Some(key_name) = key_to_name(key) {
                            events.push(KeyEvent {
                                key: key_name,
                                pressed,
                                modifiers: self.modifier_state.to_modifiers(),
                            });
                        }
                    }
                }
            }
        }

        Ok(events)
    }

    /// Check if current key + modifiers matches an activation binding
    pub fn check_activation(&self, key: &str, binding: &KeyBinding) -> bool {
        self.modifier_state.matches(binding, key)
    }

    /// Get current modifier state
    pub fn modifiers(&self) -> &ModifierState {
        &self.modifier_state
    }
}

impl Drop for InputManager {
    fn drop(&mut self) {
        if self.grabbed {
            let _ = self.ungrab();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modifier_state() {
        let mut state = ModifierState::default();
        assert!(!state.alt());
        assert!(!state.ctrl());

        state.update(KeyCode::KEY_LEFTALT, true);
        assert!(state.alt());

        state.update(KeyCode::KEY_LEFTALT, false);
        assert!(!state.alt());
    }

    #[test]
    fn test_key_binding_match() {
        let binding = KeyBinding::parse("A-M-c").unwrap();
        let mut state = ModifierState::default();
        state.left_alt = true;
        state.left_meta = true;

        assert!(state.matches(&binding, "c"));
        assert!(!state.matches(&binding, "x"));

        state.left_ctrl = true;
        assert!(!state.matches(&binding, "c"));
    }
}
