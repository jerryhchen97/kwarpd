//! KWarpd Output Manager
//!
//! Handles virtual mouse device creation and control via uinput

use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::thread;
use std::time::Duration;

// uinput constants
const UINPUT_PATH: &str = "/dev/uinput";

// Event types
const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;
const EV_REL: u16 = 0x02;

// Sync events
const SYN_REPORT: u16 = 0;

// Relative motion codes
const REL_X: u16 = 0x00;
const REL_Y: u16 = 0x01;
const REL_WHEEL: u16 = 0x08;
const REL_HWHEEL: u16 = 0x06;

// Button codes
const BTN_LEFT: u16 = 0x110;
const BTN_RIGHT: u16 = 0x111;
const BTN_MIDDLE: u16 = 0x112;

// uinput ioctl commands
const UI_SET_EVBIT: u64 = 0x40045564;
const UI_SET_KEYBIT: u64 = 0x40045565;
const UI_SET_RELBIT: u64 = 0x40045566;
const UI_DEV_CREATE: u64 = 0x5501;
const UI_DEV_DESTROY: u64 = 0x5502;

/// uinput_user_dev structure for device setup
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct UinputUserDev {
    name: [u8; 80],
    id_bustype: u16,
    id_vendor: u16,
    id_product: u16,
    id_version: u16,
    ff_effects_max: u32,
    absmax: [i32; 64],
    absmin: [i32; 64],
    absfuzz: [i32; 64],
    absflat: [i32; 64],
}

impl Default for UinputUserDev {
    fn default() -> Self {
        Self {
            name: [0u8; 80],
            id_bustype: 0x03, // BUS_USB
            id_vendor: 0x1234,
            id_product: 0x5678,
            id_version: 1,
            ff_effects_max: 0,
            absmax: [0i32; 64],
            absmin: [0i32; 64],
            absfuzz: [0i32; 64],
            absflat: [0i32; 64],
        }
    }
}

/// Input event structure
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct InputEvent {
    tv_sec: i64,
    tv_usec: i64,
    type_: u16,
    code: u16,
    value: i32,
}

impl InputEvent {
    fn new(type_: u16, code: u16, value: i32) -> Self {
        Self {
            tv_sec: 0,
            tv_usec: 0,
            type_,
            code,
            value,
        }
    }

    fn as_bytes(&self) -> &[u8] {
        bytemuck::bytes_of(self)
    }
}

/// Virtual pointer device
pub struct VirtualPointer {
    file: File,
    drag_button_held: bool,
}

impl VirtualPointer {
    /// Create a new virtual pointer device
    pub fn new() -> Result<Self> {
        let file = OpenOptions::new()
            .write(true)
            .open(UINPUT_PATH)
            .with_context(|| format!("Failed to open {}. Do you have permission?", UINPUT_PATH))?;

        let fd = file.as_raw_fd();

        // Set up event types we support
        unsafe {
            // Enable key events (for buttons)
            if libc::ioctl(fd, UI_SET_EVBIT, EV_KEY as i32) < 0 {
                anyhow::bail!("Failed to set EV_KEY");
            }
            // Enable relative motion events
            if libc::ioctl(fd, UI_SET_EVBIT, EV_REL as i32) < 0 {
                anyhow::bail!("Failed to set EV_REL");
            }

            // Set up mouse buttons
            if libc::ioctl(fd, UI_SET_KEYBIT, BTN_LEFT as i32) < 0 {
                anyhow::bail!("Failed to set BTN_LEFT");
            }
            if libc::ioctl(fd, UI_SET_KEYBIT, BTN_RIGHT as i32) < 0 {
                anyhow::bail!("Failed to set BTN_RIGHT");
            }
            if libc::ioctl(fd, UI_SET_KEYBIT, BTN_MIDDLE as i32) < 0 {
                anyhow::bail!("Failed to set BTN_MIDDLE");
            }

            // Set up relative axes
            if libc::ioctl(fd, UI_SET_RELBIT, REL_X as i32) < 0 {
                anyhow::bail!("Failed to set REL_X");
            }
            if libc::ioctl(fd, UI_SET_RELBIT, REL_Y as i32) < 0 {
                anyhow::bail!("Failed to set REL_Y");
            }
            if libc::ioctl(fd, UI_SET_RELBIT, REL_WHEEL as i32) < 0 {
                anyhow::bail!("Failed to set REL_WHEEL");
            }
            if libc::ioctl(fd, UI_SET_RELBIT, REL_HWHEEL as i32) < 0 {
                anyhow::bail!("Failed to set REL_HWHEEL");
            }
        }

        // Set up device info
        let mut dev = UinputUserDev::default();
        let name = b"kwarpd virtual pointer";
        dev.name[..name.len()].copy_from_slice(name);

        // Write device info
        let dev_bytes = bytemuck::bytes_of(&dev);

        let mut file = file;
        file.write_all(dev_bytes)
            .context("Failed to write device info")?;

        // Create the device
        unsafe {
            if libc::ioctl(file.as_raw_fd(), UI_DEV_CREATE) < 0 {
                anyhow::bail!("Failed to create uinput device");
            }
        }

        // Give the system time to register the device
        thread::sleep(Duration::from_millis(100));

        log::info!("Created virtual pointer device");

        Ok(Self {
            file,
            drag_button_held: false,
        })
    }

    /// Write an event to the device
    fn write_event(&mut self, type_: u16, code: u16, value: i32) -> Result<()> {
        let event = InputEvent::new(type_, code, value);
        self.file.write_all(event.as_bytes())?;
        Ok(())
    }

    /// Send a sync event
    fn sync(&mut self) -> Result<()> {
        self.write_event(EV_SYN, SYN_REPORT, 0)
    }

    /// Move the mouse by relative amount
    pub fn move_mouse(&mut self, dx: i32, dy: i32) -> Result<()> {
        if dx != 0 {
            self.write_event(EV_REL, REL_X, dx)?;
        }
        if dy != 0 {
            self.write_event(EV_REL, REL_Y, dy)?;
        }
        self.sync()
    }

    /// Click a mouse button (0=left, 1=middle, 2=right)
    pub fn click(&mut self, button: u8) -> Result<()> {
        let code = match button {
            0 => BTN_LEFT,
            1 => BTN_MIDDLE,
            2 => BTN_RIGHT,
            _ => anyhow::bail!("Invalid button: {}", button),
        };

        // Press
        self.write_event(EV_KEY, code, 1)?;
        self.sync()?;

        // Small delay
        thread::sleep(Duration::from_millis(10));

        // Release
        self.write_event(EV_KEY, code, 0)?;
        self.sync()
    }

    /// Press or release a mouse button
    pub fn button(&mut self, button: u8, pressed: bool) -> Result<()> {
        let code = match button {
            0 => BTN_LEFT,
            1 => BTN_MIDDLE,
            2 => BTN_RIGHT,
            _ => anyhow::bail!("Invalid button: {}", button),
        };

        self.write_event(EV_KEY, code, if pressed { 1 } else { 0 })?;
        self.sync()
    }

    /// Toggle drag mode (hold/release left button)
    pub fn toggle_drag(&mut self) -> Result<bool> {
        self.drag_button_held = !self.drag_button_held;
        self.button(0, self.drag_button_held)?;
        log::debug!("Drag mode: {}", self.drag_button_held);
        Ok(self.drag_button_held)
    }

    /// Release drag if active
    pub fn release_drag(&mut self) -> Result<()> {
        if self.drag_button_held {
            self.drag_button_held = false;
            self.button(0, false)?;
        }
        Ok(())
    }

    /// Scroll the mouse wheel
    pub fn scroll(&mut self, amount: i32) -> Result<()> {
        // Negative amount = scroll up, positive = scroll down
        self.write_event(EV_REL, REL_WHEEL, -amount)?;
        self.sync()
    }

    /// Horizontal scroll
    pub fn hscroll(&mut self, amount: i32) -> Result<()> {
        self.write_event(EV_REL, REL_HWHEEL, amount)?;
        self.sync()
    }

    /// Check if drag is active
    pub fn is_dragging(&self) -> bool {
        self.drag_button_held
    }
}

impl Drop for VirtualPointer {
    fn drop(&mut self) {
        // Release any held buttons
        let _ = self.release_drag();

        // Destroy the device
        unsafe {
            libc::ioctl(self.file.as_raw_fd(), UI_DEV_DESTROY);
        }
        log::info!("Destroyed virtual pointer device");
    }
}

#[cfg(test)]
mod tests {
    // Note: These tests require root/uinput permissions to run
    // They are here for documentation purposes

    #[test]
    #[ignore]
    fn test_create_virtual_pointer() {
        use super::*;
        let pointer = VirtualPointer::new();
        assert!(pointer.is_ok());
    }
}
