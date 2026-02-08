//! KWarpd Configuration Module
//!
//! Handles loading and parsing configuration from ~/.config/kwarpd/kwarpd.conf

use anyhow::{Context, Result};
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

/// Modifier keys that can be combined with other keys
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
    pub super_key: bool,
}

/// A key binding with optional modifiers
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyBinding {
    pub modifiers: Modifiers,
    pub key: String,
}

impl KeyBinding {
    /// Parse a key binding from a string like "A-M-c" (Alt+Meta+c)
    /// Modifiers: A = Alt, C = Control, S = Shift, M = Meta/Super
    pub fn parse(s: &str) -> Result<Self> {
        let parts: Vec<&str> = s.split('-').collect();
        let mut modifiers = Modifiers::default();
        let mut key = String::new();

        for (i, part) in parts.iter().enumerate() {
            if i == parts.len() - 1 {
                // Last part is the key
                key = part.to_lowercase();
            } else {
                // Modifier
                match *part {
                    "A" => modifiers.alt = true,
                    "C" => modifiers.ctrl = true,
                    "S" => modifiers.shift = true,
                    "M" => modifiers.super_key = true,
                    _ => anyhow::bail!("Unknown modifier: {}", part),
                }
            }
        }

        if key.is_empty() {
            anyhow::bail!("No key specified in binding: {}", s);
        }

        Ok(Self { modifiers, key })
    }
}

/// Mouse buttons configuration
#[derive(Debug, Clone)]
pub struct MouseButtons {
    pub left: String,
    pub middle: String,
    pub right: String,
}

impl Default for MouseButtons {
    fn default() -> Self {
        Self {
            left: "m".to_string(),
            middle: ",".to_string(),
            right: ".".to_string(),
        }
    }
}

/// Raw configuration as deserialized from TOML
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct RawConfig {
    // Activation keys
    hint_activation_key: Option<String>,
    activation_key: Option<String>,

    // Mode control
    exit: Option<String>,
    drag: Option<String>,
    copy_and_exit: Option<String>,
    hint: Option<String>,

    // Movement modifiers
    accelerator: Option<String>,
    decelerator: Option<String>,

    // Mouse buttons (space-separated)
    buttons: Option<String>,

    // Movement keys
    left: Option<String>,
    down: Option<String>,
    up: Option<String>,
    right: Option<String>,

    // Scrolling
    scroll_down: Option<String>,
    scroll_up: Option<String>,

    // Visual settings
    cursor_color: Option<String>,
    cursor_size: Option<u32>,

    // Movement physics
    speed: Option<u32>,
    max_speed: Option<u32>,
    decelerator_speed: Option<u32>,
    acceleration: Option<u32>,
    accelerator_acceleration: Option<u32>,

    // Hint mode settings
    hint_chars: Option<String>,
    hint_size: Option<u32>,
    hint_exit: Option<String>,

    // Scroll physics
    scroll_speed: Option<u32>,
    scroll_max_speed: Option<u32>,
    scroll_acceleration: Option<u32>,
    scroll_deceleration: Option<i32>,
}

/// Parsed and validated configuration
#[derive(Debug, Clone)]
pub struct Config {
    // Activation keys
    pub hint_activation_key: KeyBinding,
    pub activation_key: KeyBinding,

    // Mode control
    pub exit: String,
    pub drag: String,
    pub copy_and_exit: String,
    pub hint: String,

    // Movement modifiers
    pub accelerator: String,
    pub decelerator: String,

    // Mouse buttons
    pub buttons: MouseButtons,

    // Movement keys
    pub left: String,
    pub down: String,
    pub up: String,
    pub right: String,

    // Scrolling keys
    pub scroll_down: String,
    pub scroll_up: String,

    // Visual settings
    pub cursor_color: u32, // RGBA
    pub cursor_size: u32,

    // Movement physics
    pub speed: u32,
    pub max_speed: u32,
    pub decelerator_speed: u32,
    pub acceleration: u32,
    pub accelerator_acceleration: u32,

    // Hint mode settings
    pub hint_chars: String,
    pub hint_size: u32,
    pub hint_exit: String,

    // Scroll physics
    pub scroll_speed: u32,
    pub scroll_max_speed: u32,
    pub scroll_acceleration: u32,
    pub scroll_deceleration: i32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hint_activation_key: KeyBinding::parse("A-M-x").unwrap(),
            activation_key: KeyBinding::parse("A-M-c").unwrap(),
            exit: "esc".to_string(),
            drag: "v".to_string(),
            copy_and_exit: "c".to_string(),
            hint: "x".to_string(),
            accelerator: "a".to_string(),
            decelerator: "d".to_string(),
            buttons: MouseButtons::default(),
            left: "h".to_string(),
            down: "j".to_string(),
            up: "k".to_string(),
            right: "l".to_string(),
            scroll_down: "e".to_string(),
            scroll_up: "r".to_string(),
            cursor_color: 0xFF4500FF, // #FF4500 (OrangeRed) with full alpha
            cursor_size: 7,
            speed: 220,
            max_speed: 1600,
            decelerator_speed: 50,
            acceleration: 700,
            accelerator_acceleration: 2900,
            hint_chars: "abcdefghijklmnopqrstuvwxyz".to_string(),
            hint_size: 20,
            hint_exit: "esc".to_string(),
            scroll_speed: 300,
            scroll_max_speed: 9000,
            scroll_acceleration: 1600,
            scroll_deceleration: -3400,
        }
    }
}

impl Config {
    /// Get the default config file path
    pub fn default_path() -> Option<PathBuf> {
        dirs::config_dir().map(|p| p.join("kwarpd").join("kwarpd.conf"))
    }

    /// Load configuration from file, falling back to defaults
    pub fn load() -> Result<Self> {
        let path = Self::default_path();

        if let Some(ref p) = path {
            if p.exists() {
                return Self::load_from_file(p);
            }
        }

        log::info!("No config file found, using defaults");
        Ok(Self::default())
    }

    /// Load configuration from a specific file
    pub fn load_from_file(path: &PathBuf) -> Result<Self> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {:?}", path))?;

        Self::parse(&content)
    }

    /// Parse configuration from TOML string
    pub fn parse(content: &str) -> Result<Self> {
        let raw: RawConfig = toml::from_str(content)
            .with_context(|| "Failed to parse config file")?;

        let mut config = Self::default();

        // Parse activation keys
        if let Some(ref s) = raw.hint_activation_key {
            config.hint_activation_key = KeyBinding::parse(s)
                .with_context(|| format!("Invalid hint_activation_key: {}", s))?;
        }
        if let Some(ref s) = raw.activation_key {
            config.activation_key = KeyBinding::parse(s)
                .with_context(|| format!("Invalid activation_key: {}", s))?;
        }

        // Simple string options
        if let Some(s) = raw.exit { config.exit = s; }
        if let Some(s) = raw.drag { config.drag = s; }
        if let Some(s) = raw.copy_and_exit { config.copy_and_exit = s; }
        if let Some(s) = raw.hint { config.hint = s; }
        if let Some(s) = raw.accelerator { config.accelerator = s; }
        if let Some(s) = raw.decelerator { config.decelerator = s; }
        if let Some(s) = raw.left { config.left = s; }
        if let Some(s) = raw.down { config.down = s; }
        if let Some(s) = raw.up { config.up = s; }
        if let Some(s) = raw.right { config.right = s; }
        if let Some(s) = raw.scroll_down { config.scroll_down = s; }
        if let Some(s) = raw.scroll_up { config.scroll_up = s; }
        if let Some(s) = raw.hint_chars { config.hint_chars = s; }
        if let Some(s) = raw.hint_exit { config.hint_exit = s; }

        // Parse buttons (space-separated: "m , .")
        if let Some(ref s) = raw.buttons {
            let parts: Vec<&str> = s.split_whitespace().collect();
            if parts.len() >= 3 {
                config.buttons = MouseButtons {
                    left: parts[0].to_string(),
                    middle: parts[1].to_string(),
                    right: parts[2].to_string(),
                };
            }
        }

        // Parse cursor color (#RRGGBB -> RGBA)
        if let Some(ref s) = raw.cursor_color {
            config.cursor_color = parse_color(s)
                .with_context(|| format!("Invalid cursor_color: {}", s))?;
        }

        // Numeric options
        if let Some(v) = raw.cursor_size { config.cursor_size = v; }
        if let Some(v) = raw.speed { config.speed = v; }
        if let Some(v) = raw.max_speed { config.max_speed = v; }
        if let Some(v) = raw.decelerator_speed { config.decelerator_speed = v; }
        if let Some(v) = raw.acceleration { config.acceleration = v; }
        if let Some(v) = raw.accelerator_acceleration { config.accelerator_acceleration = v; }
        if let Some(v) = raw.hint_size { config.hint_size = v; }
        if let Some(v) = raw.scroll_speed { config.scroll_speed = v; }
        if let Some(v) = raw.scroll_max_speed { config.scroll_max_speed = v; }
        if let Some(v) = raw.scroll_acceleration { config.scroll_acceleration = v; }
        if let Some(v) = raw.scroll_deceleration { config.scroll_deceleration = v; }

        Ok(config)
    }
}

/// Parse a color string like "#FF4500" or "#FF4500FF" into RGBA u32
fn parse_color(s: &str) -> Result<u32> {
    let s = s.trim_start_matches('#');

    match s.len() {
        6 => {
            // RGB -> RGBA (add FF alpha)
            let rgb = u32::from_str_radix(s, 16)
                .with_context(|| "Invalid hex color")?;
            Ok((rgb << 8) | 0xFF)
        }
        8 => {
            // RGBA
            u32::from_str_radix(s, 16)
                .with_context(|| "Invalid hex color")
        }
        _ => anyhow::bail!("Color must be 6 or 8 hex digits"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_key_binding() {
        let kb = KeyBinding::parse("A-M-c").unwrap();
        assert!(kb.modifiers.alt);
        assert!(kb.modifiers.super_key);
        assert!(!kb.modifiers.ctrl);
        assert!(!kb.modifiers.shift);
        assert_eq!(kb.key, "c");
    }

    #[test]
    fn test_parse_simple_key() {
        let kb = KeyBinding::parse("esc").unwrap();
        assert!(!kb.modifiers.alt);
        assert_eq!(kb.key, "esc");
    }

    #[test]
    fn test_parse_color() {
        assert_eq!(parse_color("#FF4500").unwrap(), 0xFF4500FF);
        assert_eq!(parse_color("#FF450080").unwrap(), 0xFF450080);
    }

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.speed, 220);
        assert_eq!(config.hint_chars, "abcdefghijklmnopqrstuvwxyz");
    }

    #[test]
    fn test_parse_config() {
        let toml = r#"
            speed = 300
            left = "a"
            activation_key = "C-M-k"
        "#;
        let config = Config::parse(toml).unwrap();
        assert_eq!(config.speed, 300);
        assert_eq!(config.left, "a");
        assert!(config.activation_key.modifiers.ctrl);
        assert!(config.activation_key.modifiers.super_key);
    }
}
