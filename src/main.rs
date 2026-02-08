//! KWarpd - Keyboard-driven cursor manipulation for KWin on Wayland
//!
//! A modal keyboard-driven cursor manipulation tool inspired by warpd

mod config;
mod input;
mod output;
mod overlay;
mod state;

use anyhow::{Context, Result};
use clap::Parser;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::input::InputManager;
use crate::output::VirtualPointer;
use crate::overlay::{calculate_hints, find_hint_exact, find_hint_by_prefix, HintPoint};
use crate::state::{Action, AppState, Mode};

/// Command-line arguments
#[derive(Parser, Debug)]
#[command(name = "kwarpd", version, about = "Keyboard-driven cursor manipulation for KWin on Wayland")]
struct Args {
    /// Path to configuration file
    #[arg(short, long)]
    config: Option<String>,

    /// Run in foreground mode (don't daemonize)
    #[arg(short, long)]
    foreground: bool,

    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,
}

/// Physics state for smooth movement
struct PhysicsState {
    velocity_x: f64,
    velocity_y: f64,
    scroll_velocity: f64,
    last_update: Instant,
}

impl PhysicsState {
    fn new() -> Self {
        Self {
            velocity_x: 0.0,
            velocity_y: 0.0,
            scroll_velocity: 0.0,
            last_update: Instant::now(),
        }
    }

    fn reset(&mut self) {
        self.velocity_x = 0.0;
        self.velocity_y = 0.0;
        self.scroll_velocity = 0.0;
        self.last_update = Instant::now();
    }

    /// Update physics and return movement delta
    fn update(&mut self, state: &AppState, config: &Config) -> (i32, i32, i32) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_update).as_secs_f64();
        self.last_update = now;

        if dt <= 0.0 || dt > 0.1 {
            // Skip if time delta is too large (probably first frame)
            return (0, 0, 0);
        }

        let (dir_x, dir_y) = state.movement.direction();
        let scroll_dir = state.scroll.direction();

        // Select acceleration based on modifier keys
        let accel = if state.movement.accelerating {
            config.accelerator_acceleration as f64
        } else if state.movement.decelerating {
            0.0 // No acceleration when decelerating
        } else {
            config.acceleration as f64
        };

        // Select target speed
        let target_speed = if state.movement.decelerating {
            config.decelerator_speed as f64
        } else if state.movement.accelerating {
            config.max_speed as f64
        } else {
            config.speed as f64
        };

        // Calculate target velocity
        let target_vx = dir_x as f64 * target_speed;
        let target_vy = dir_y as f64 * target_speed;

        // Apply acceleration/deceleration
        if dir_x != 0 || dir_y != 0 {
            // Accelerate towards target
            let accel_step = accel * dt;
            self.velocity_x = move_towards(self.velocity_x, target_vx, accel_step);
            self.velocity_y = move_towards(self.velocity_y, target_vy, accel_step);
        } else {
            // Decelerate to stop
            let decel_step = config.acceleration as f64 * dt * 2.0;
            self.velocity_x = move_towards(self.velocity_x, 0.0, decel_step);
            self.velocity_y = move_towards(self.velocity_y, 0.0, decel_step);
        }

        // Clamp to max speed
        let max = config.max_speed as f64;
        self.velocity_x = self.velocity_x.clamp(-max, max);
        self.velocity_y = self.velocity_y.clamp(-max, max);

        // Calculate scroll velocity
        if scroll_dir != 0 {
            let target_scroll = scroll_dir as f64 * config.scroll_max_speed as f64;
            let scroll_accel = config.scroll_acceleration as f64 * dt;
            self.scroll_velocity = move_towards(self.scroll_velocity, target_scroll, scroll_accel);
        } else {
            let scroll_decel = config.scroll_deceleration.unsigned_abs() as f64 * dt;
            self.scroll_velocity = move_towards(self.scroll_velocity, 0.0, scroll_decel);
        }

        // Calculate movement deltas
        let dx = (self.velocity_x * dt).round() as i32;
        let dy = (self.velocity_y * dt).round() as i32;
        let scroll = (self.scroll_velocity * dt / 100.0).round() as i32; // Scale scroll

        (dx, dy, scroll)
    }
}

/// Move a value towards target by delta
fn move_towards(current: f64, target: f64, delta: f64) -> f64 {
    if current < target {
        (current + delta).min(target)
    } else if current > target {
        (current - delta).max(target)
    } else {
        current
    }
}

/// Main application loop
fn run(config: Config) -> Result<()> {
    let config = Arc::new(config);

    // Initialize input manager
    let mut input = InputManager::new()
        .context("Failed to initialize input manager")?;

    // Initialize virtual pointer
    let mut pointer = VirtualPointer::new()
        .context("Failed to initialize virtual pointer")?;

    // Application state
    let mut state = AppState::new();
    let mut physics = PhysicsState::new();

    // Hint state (used in hint mode)
    let mut hints: Vec<HintPoint> = Vec::new();
    let mut screen_width: u32 = 1920; // Default, would be queried from Wayland
    let mut screen_height: u32 = 1080;

    log::info!("kwarpd started, waiting for activation key...");
    log::info!("Normal mode: {:?}", config.activation_key);
    log::info!("Hint mode: {:?}", config.hint_activation_key);

    // Main loop
    let frame_duration = Duration::from_millis(16); // ~60 FPS

    loop {
        let frame_start = Instant::now();

        // Poll for input events
        let events = input.poll_events().unwrap_or_default();

        for event in events {
            match state.mode {
                Mode::Inactive => {
                    // Check for activation keys
                    if event.pressed {
                        if input.check_activation(&event.key, &config.activation_key) {
                            log::info!("Entering Normal mode");
                            state.enter_normal();
                            input.grab()?;
                            physics.reset();
                        } else if input.check_activation(&event.key, &config.hint_activation_key) {
                            log::info!("Entering Hint mode");
                            state.enter_hint();
                            input.grab()?;
                            // Generate hints
                            hints = calculate_hints(
                                screen_width,
                                screen_height,
                                &config.hint_chars,
                                config.hint_size,
                            );
                            // TODO: Show overlay via Wayland
                        }
                    }
                }

                Mode::Normal | Mode::Hint => {
                    let action = state.process_key(&event.key, event.pressed, &config);

                    match action {
                        Action::Exit => {
                            log::info!("Exiting mode");
                            state.exit();
                            input.ungrab()?;
                            physics.reset();
                            pointer.release_drag()?;
                            hints.clear();
                        }

                        Action::EnterHint => {
                            log::info!("Switching to Hint mode");
                            state.enter_hint();
                            hints = calculate_hints(
                                screen_width,
                                screen_height,
                                &config.hint_chars,
                                config.hint_size,
                            );
                        }

                        Action::EnterNormal => {
                            log::info!("Switching to Normal mode");
                            state.enter_normal();
                            hints.clear();
                        }

                        Action::Click(button) => {
                            log::debug!("Click button {}", button);
                            pointer.click(button)?;
                        }

                        Action::ToggleDrag => {
                            let dragging = pointer.toggle_drag()?;
                            log::info!("Drag mode: {}", if dragging { "on" } else { "off" });
                        }

                        Action::CopyAndExit => {
                            // Send Ctrl+C via uinput would be complex,
                            // for now just exit
                            log::info!("Copy and exit (Ctrl+C not implemented)");
                            state.exit();
                            input.ungrab()?;
                            pointer.release_drag()?;
                        }

                        Action::HintChar(ch) => {
                            log::debug!("Hint char: {}", ch);
                            let buffer = &state.hint_buffer;

                            // Check for exact match
                            if let Some(hint) = find_hint_exact(&hints, buffer) {
                                log::info!("Hint matched: {} -> ({}, {})", buffer, hint.x, hint.y);
                                // TODO: Warp cursor to hint position (requires absolute positioning)
                                // For now, print the position
                                state.exit();
                                input.ungrab()?;
                                hints.clear();
                            } else {
                                // Check if any hints match the prefix
                                let matches = find_hint_by_prefix(&hints, buffer);
                                if matches.is_empty() {
                                    log::debug!("No hints match prefix: {}", buffer);
                                    state.hint_buffer.clear();
                                }
                            }
                        }

                        _ => {}
                    }
                }
            }
        }

        // Update physics and move pointer (only in normal mode with movement)
        if state.mode == Mode::Normal {
            let (dx, dy, scroll) = physics.update(&state, &config);

            if dx != 0 || dy != 0 {
                pointer.move_mouse(dx, dy)?;
            }

            if scroll != 0 {
                pointer.scroll(scroll)?;
            }
        }

        // Frame rate limiting
        let elapsed = frame_start.elapsed();
        if elapsed < frame_duration {
            thread::sleep(frame_duration - elapsed);
        }
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    env_logger::Builder::from_default_env()
        .filter_level(if args.debug {
            log::LevelFilter::Debug
        } else {
            log::LevelFilter::Info
        })
        .init();

    // Load configuration
    let config = if let Some(ref path) = args.config {
        Config::load_from_file(&std::path::PathBuf::from(path))?
    } else {
        Config::load()?
    };

    log::debug!("Configuration loaded: {:?}", config);

    // Run the main loop
    run(config)
}
