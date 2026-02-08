//! KWarpd State Machine
//!
//! Defines the application modes and state transitions

use crate::config::Config;

/// The current mode of the application
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Not active - listening for activation keys only
    Inactive,
    /// Normal mode - keyboard controls mouse movement
    Normal,
    /// Hint mode - overlay is shown, waiting for hint input
    Hint,
}

/// Actions that can be performed based on input
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// No action
    None,
    /// Enter normal mode
    EnterNormal,
    /// Enter hint mode
    EnterHint,
    /// Exit to inactive state
    Exit,
    /// Move cursor in direction (dx, dy normalized)
    Move { dx: i32, dy: i32 },
    /// Click a mouse button (0=left, 1=middle, 2=right)
    Click(u8),
    /// Toggle drag mode
    ToggleDrag,
    /// Send copy key and exit
    CopyAndExit,
    /// Scroll (dy: positive=down, negative=up)
    Scroll(i32),
    /// Hint character typed
    HintChar(char),
    /// Apply accelerator (multiply speed)
    Accelerate,
    /// Apply decelerator (reduce speed)
    Decelerate,
    /// Stop acceleration/deceleration
    ReleaseSpeedMod,
}

/// Movement direction state
#[derive(Debug, Clone, Default)]
pub struct MovementState {
    pub left: bool,
    pub right: bool,
    pub up: bool,
    pub down: bool,
    pub accelerating: bool,
    pub decelerating: bool,
}

impl MovementState {
    /// Get normalized direction vector
    pub fn direction(&self) -> (i32, i32) {
        let dx = if self.left { -1 } else { 0 } + if self.right { 1 } else { 0 };
        let dy = if self.up { -1 } else { 0 } + if self.down { 1 } else { 0 };
        (dx, dy)
    }

    /// Check if any movement key is pressed
    pub fn is_moving(&self) -> bool {
        self.left || self.right || self.up || self.down
    }
}

/// Scroll direction state
#[derive(Debug, Clone, Default)]
pub struct ScrollState {
    pub up: bool,
    pub down: bool,
}

impl ScrollState {
    /// Get scroll direction (-1 for up, 1 for down, 0 for none)
    pub fn direction(&self) -> i32 {
        if self.down { 1 } else if self.up { -1 } else { 0 }
    }

    /// Check if scrolling
    pub fn is_scrolling(&self) -> bool {
        self.up || self.down
    }
}

/// Application state
#[derive(Debug)]
pub struct AppState {
    pub mode: Mode,
    pub drag_active: bool,
    pub movement: MovementState,
    pub scroll: ScrollState,
    pub hint_buffer: String,
    pub current_speed: f64,
    pub current_scroll_speed: f64,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            mode: Mode::Inactive,
            drag_active: false,
            movement: MovementState::default(),
            scroll: ScrollState::default(),
            hint_buffer: String::new(),
            current_speed: 0.0,
            current_scroll_speed: 0.0,
        }
    }
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset state when exiting a mode
    pub fn reset(&mut self) {
        self.movement = MovementState::default();
        self.scroll = ScrollState::default();
        self.hint_buffer.clear();
        self.current_speed = 0.0;
        self.current_scroll_speed = 0.0;
    }

    /// Enter normal mode
    pub fn enter_normal(&mut self) {
        self.reset();
        self.mode = Mode::Normal;
    }

    /// Enter hint mode
    pub fn enter_hint(&mut self) {
        self.reset();
        self.mode = Mode::Hint;
    }

    /// Exit to inactive
    pub fn exit(&mut self) {
        self.reset();
        self.mode = Mode::Inactive;
        self.drag_active = false;
    }

    /// Process a key and return the action
    pub fn process_key(&mut self, key: &str, pressed: bool, config: &Config) -> Action {
        match self.mode {
            Mode::Inactive => Action::None, // Activation handled elsewhere
            Mode::Normal => self.process_normal_key(key, pressed, config),
            Mode::Hint => self.process_hint_key(key, pressed, config),
        }
    }

    fn process_normal_key(&mut self, key: &str, pressed: bool, config: &Config) -> Action {
        // Handle key releases for movement
        if !pressed {
            if key == config.accelerator {
                self.movement.accelerating = false;
                return Action::ReleaseSpeedMod;
            }
            if key == config.decelerator {
                self.movement.decelerating = false;
                return Action::ReleaseSpeedMod;
            }
            // Handle direction key releases
            if key == config.left { self.movement.left = false; }
            if key == config.right { self.movement.right = false; }
            if key == config.up { self.movement.up = false; }
            if key == config.down { self.movement.down = false; }
            if key == config.scroll_up { self.scroll.up = false; }
            if key == config.scroll_down { self.scroll.down = false; }
            return Action::None;
        }

        // Key presses
        if key == config.exit {
            return Action::Exit;
        }
        if key == config.hint {
            return Action::EnterHint;
        }
        if key == config.drag {
            self.drag_active = !self.drag_active;
            return Action::ToggleDrag;
        }
        if key == config.copy_and_exit {
            return Action::CopyAndExit;
        }
        if key == config.accelerator && !self.movement.accelerating {
            self.movement.accelerating = true;
            return Action::Accelerate;
        }
        if key == config.decelerator && !self.movement.decelerating {
            self.movement.decelerating = true;
            return Action::Decelerate;
        }

        // Movement keys
        if key == config.left && !self.movement.left {
            self.movement.left = true;
            let (dx, dy) = self.movement.direction();
            return Action::Move { dx, dy };
        }
        if key == config.right && !self.movement.right {
            self.movement.right = true;
            let (dx, dy) = self.movement.direction();
            return Action::Move { dx, dy };
        }
        if key == config.up && !self.movement.up {
            self.movement.up = true;
            let (dx, dy) = self.movement.direction();
            return Action::Move { dx, dy };
        }
        if key == config.down && !self.movement.down {
            self.movement.down = true;
            let (dx, dy) = self.movement.direction();
            return Action::Move { dx, dy };
        }

        // Scroll keys
        if key == config.scroll_up && !self.scroll.up {
            self.scroll.up = true;
            return Action::Scroll(-1);
        }
        if key == config.scroll_down && !self.scroll.down {
            self.scroll.down = true;
            return Action::Scroll(1);
        }

        // Mouse buttons
        if key == config.buttons.left {
            return Action::Click(0);
        }
        if key == config.buttons.middle {
            return Action::Click(1);
        }
        if key == config.buttons.right {
            return Action::Click(2);
        }

        Action::None
    }

    fn process_hint_key(&mut self, key: &str, pressed: bool, config: &Config) -> Action {
        if !pressed {
            return Action::None;
        }

        if key == config.hint_exit || key == config.exit {
            return Action::Exit;
        }

        // Check if it's a valid hint character
        if key.len() == 1 {
            let ch = key.chars().next().unwrap();
            if config.hint_chars.contains(ch) {
                self.hint_buffer.push(ch);
                return Action::HintChar(ch);
            }
        }

        // Backspace to clear buffer
        if key == "backspace" {
            self.hint_buffer.pop();
        }

        Action::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_movement_direction() {
        let mut m = MovementState::default();
        assert_eq!(m.direction(), (0, 0));

        m.left = true;
        assert_eq!(m.direction(), (-1, 0));

        m.up = true;
        assert_eq!(m.direction(), (-1, -1));

        m.right = true;
        // left and right cancel out
        assert_eq!(m.direction(), (0, -1));
    }

    #[test]
    fn test_state_transitions() {
        let mut state = AppState::new();
        assert_eq!(state.mode, Mode::Inactive);

        state.enter_normal();
        assert_eq!(state.mode, Mode::Normal);

        state.enter_hint();
        assert_eq!(state.mode, Mode::Hint);

        state.exit();
        assert_eq!(state.mode, Mode::Inactive);
    }
}
