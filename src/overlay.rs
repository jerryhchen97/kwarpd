//! KWarpd Overlay Module
//!
//! Handles the Wayland overlay window for hint mode using layer-shell

use anyhow::{Context, Result};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_layer, delegate_output, delegate_registry, delegate_shm,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use std::sync::Arc;
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_shm, wl_surface},
    Connection, QueueHandle,
};

use crate::config::Config;

/// Hint point on the screen
#[derive(Debug, Clone)]
pub struct HintPoint {
    pub x: i32,
    pub y: i32,
    pub label: String,
}

/// Calculate hint grid positions
pub fn calculate_hints(width: u32, height: u32, hint_chars: &str, _hint_size: u32) -> Vec<HintPoint> {
    let chars: Vec<char> = hint_chars.chars().collect();
    let num_chars = chars.len();

    // For two-character hints, we have num_chars^2 total hints
    let total_hints = num_chars * num_chars;

    // Calculate grid dimensions
    let aspect = width as f64 / height as f64;
    let hints_y = ((total_hints as f64) / aspect).sqrt().ceil() as u32;
    let hints_x = ((total_hints as f64) * aspect).sqrt().ceil() as u32;

    let spacing_x = width / (hints_x + 1);
    let spacing_y = height / (hints_y + 1);

    let mut hints = Vec::new();
    let mut hint_idx = 0;

    for row in 0..hints_y {
        for col in 0..hints_x {
            if hint_idx >= total_hints {
                break;
            }

            let x = ((col + 1) * spacing_x) as i32;
            let y = ((row + 1) * spacing_y) as i32;

            // Generate two-character label
            let first_char = chars[hint_idx / num_chars];
            let second_char = chars[hint_idx % num_chars];
            let label = format!("{}{}", first_char, second_char);

            hints.push(HintPoint { x, y, label });
            hint_idx += 1;
        }
    }

    hints
}

/// Find a hint by its label prefix
pub fn find_hint_by_prefix<'a>(hints: &'a [HintPoint], prefix: &str) -> Vec<&'a HintPoint> {
    hints.iter().filter(|h| h.label.starts_with(prefix)).collect()
}

/// Find exact hint match
pub fn find_hint_exact<'a>(hints: &'a [HintPoint], label: &str) -> Option<&'a HintPoint> {
    hints.iter().find(|h| h.label == label)
}

/// Draw hints onto a pixel buffer (ARGB8888 format)
/// This is a simplified version that draws directly to the buffer without tiny-skia conflicts
pub fn draw_hints(
    buffer: &mut [u8],
    width: u32,
    height: u32,
    hints: &[HintPoint],
    highlight_prefix: &str,
    hint_size: u32,
    font_data: &[u8],
) {
    use fontdue::{Font, FontSettings};

    // Clear buffer to transparent
    buffer.fill(0);

    // Load font
    let font = match Font::from_bytes(font_data, FontSettings::default()) {
        Ok(f) => f,
        Err(_) => return,
    };

    let font_size = hint_size as f32;

    for hint in hints {
        let is_highlighted = !highlight_prefix.is_empty() && hint.label.starts_with(highlight_prefix);
        let is_filtered_out = !highlight_prefix.is_empty() && !hint.label.starts_with(highlight_prefix);

        if is_filtered_out {
            continue;
        }

        // Colors (BGRA format for ARGB8888)
        let (bg_b, bg_g, bg_r, bg_a) = if is_highlighted {
            (0u8, 200u8, 255u8, 230u8) // Yellow
        } else {
            (40u8, 40u8, 40u8, 220u8) // Dark gray
        };

        let (txt_b, txt_g, txt_r) = if is_highlighted {
            (0u8, 0u8, 0u8) // Black
        } else {
            (255u8, 255u8, 255u8) // White
        };

        let text_width = (hint.label.len() as f32 * font_size * 0.6) as i32;
        let text_height = font_size as i32;
        let padding = 4i32;

        let rect_x = hint.x - text_width / 2 - padding;
        let rect_y = hint.y - text_height / 2 - padding;
        let rect_w = text_width + padding * 2;
        let rect_h = text_height + padding * 2;

        // Draw background rectangle
        for py in rect_y.max(0)..(rect_y + rect_h).min(height as i32) {
            for px in rect_x.max(0)..(rect_x + rect_w).min(width as i32) {
                let idx = ((py as u32 * width + px as u32) * 4) as usize;
                if idx + 3 < buffer.len() {
                    buffer[idx] = bg_b;
                    buffer[idx + 1] = bg_g;
                    buffer[idx + 2] = bg_r;
                    buffer[idx + 3] = bg_a;
                }
            }
        }

        // Draw text
        let mut cursor_x = hint.x - text_width / 2;
        let cursor_y = hint.y + text_height / 4;

        for ch in hint.label.chars() {
            let (metrics, bitmap) = font.rasterize(ch, font_size);

            if !bitmap.is_empty() {
                for gy in 0..metrics.height {
                    for gx in 0..metrics.width {
                        let alpha = bitmap[gy * metrics.width + gx];
                        if alpha > 0 {
                            let px = cursor_x + gx as i32;
                            let py = cursor_y - metrics.height as i32 + gy as i32;

                            if px >= 0 && px < width as i32 && py >= 0 && py < height as i32 {
                                let idx = ((py as u32 * width + px as u32) * 4) as usize;
                                if idx + 3 < buffer.len() {
                                    let a = alpha as f32 / 255.0;
                                    buffer[idx] = ((1.0 - a) * buffer[idx] as f32 + a * txt_b as f32) as u8;
                                    buffer[idx + 1] = ((1.0 - a) * buffer[idx + 1] as f32 + a * txt_g as f32) as u8;
                                    buffer[idx + 2] = ((1.0 - a) * buffer[idx + 2] as f32 + a * txt_r as f32) as u8;
                                    buffer[idx + 3] = 255;
                                }
                            }
                        }
                    }
                }
            }

            cursor_x += metrics.advance_width as i32;
        }
    }
}

/// Overlay application state for Wayland
pub struct OverlayApp {
    registry_state: RegistryState,
    shm: Shm,
    pool: Option<SlotPool>,
    layer_shell: LayerShell,
    compositor: CompositorState,
    output_state: OutputState,
    layer_surface: Option<LayerSurface>,
    width: u32,
    height: u32,
    configured: bool,
    hints: Vec<HintPoint>,
    highlight_prefix: String,
    config: Arc<Config>,
    font_data: Vec<u8>,
    should_close: bool,
}

impl OverlayApp {
    pub fn new(
        conn: &Connection,
        qh: &QueueHandle<Self>,
        config: Arc<Config>,
    ) -> Result<Self> {
        let (globals, _) = registry_queue_init::<Self>(conn)
            .context("Failed to initialize Wayland registry")?;

        let registry_state = RegistryState::new(&globals);
        let shm = Shm::bind(&globals, qh).context("Failed to bind wl_shm")?;
        let compositor = CompositorState::bind(&globals, qh)
            .context("Failed to bind wl_compositor")?;
        let layer_shell = LayerShell::bind(&globals, qh)
            .context("Failed to bind zwlr_layer_shell_v1. Is your compositor compatible?")?;
        let output_state = OutputState::new(&globals, qh);

        // Load embedded font
        let font_data = include_bytes!("../assets/font.ttf").to_vec();

        Ok(Self {
            registry_state,
            shm,
            pool: None,
            layer_shell,
            compositor,
            output_state,
            layer_surface: None,
            width: 0,
            height: 0,
            configured: false,
            hints: Vec::new(),
            highlight_prefix: String::new(),
            config,
            font_data,
            should_close: false,
        })
    }

    /// Create and show the overlay surface
    pub fn show(&mut self, qh: &QueueHandle<Self>) -> Result<()> {
        let surface = self.compositor.create_surface(qh);

        let layer_surface = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Overlay,
            Some("kwarpd-hints"),
            None,
        );

        layer_surface.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer_surface.set_exclusive_zone(-1);
        layer_surface.set_keyboard_interactivity(KeyboardInteractivity::None);
        layer_surface.commit();

        self.layer_surface = Some(layer_surface);
        Ok(())
    }

    /// Hide and destroy the overlay
    pub fn hide(&mut self) {
        self.layer_surface = None;
        self.configured = false;
        self.hints.clear();
        self.highlight_prefix.clear();
    }

    /// Set hints to display
    pub fn set_hints(&mut self, hints: Vec<HintPoint>) {
        self.hints = hints;
        self.highlight_prefix.clear();
    }

    /// Update highlight prefix
    pub fn set_highlight(&mut self, prefix: &str) {
        self.highlight_prefix = prefix.to_string();
    }

    /// Check if overlay is shown
    pub fn is_shown(&self) -> bool {
        self.layer_surface.is_some() && self.configured
    }

    /// Request close
    pub fn request_close(&mut self) {
        self.should_close = true;
    }

    /// Check if close was requested
    pub fn should_close(&self) -> bool {
        self.should_close
    }

    /// Draw the overlay
    fn draw(&mut self, _qh: &QueueHandle<Self>) {
        if !self.configured || self.layer_surface.is_none() {
            return;
        }

        let layer_surface = self.layer_surface.as_ref().unwrap();

        let stride = self.width * 4;
        let size = (stride * self.height) as usize;

        if self.pool.is_none() {
            self.pool = SlotPool::new(size, &self.shm).ok();
        }

        let pool = match &mut self.pool {
            Some(p) => p,
            None => return,
        };

        let (buffer, canvas) = match pool.create_buffer(
            self.width as i32,
            self.height as i32,
            stride as i32,
            wl_shm::Format::Argb8888,
        ) {
            Ok((b, c)) => (b, c),
            Err(_) => return,
        };

        draw_hints(
            canvas,
            self.width,
            self.height,
            &self.hints,
            &self.highlight_prefix,
            self.config.hint_size,
            &self.font_data,
        );

        layer_surface.wl_surface().attach(Some(buffer.wl_buffer()), 0, 0);
        layer_surface.wl_surface().damage_buffer(0, 0, self.width as i32, self.height as i32);
        layer_surface.commit();
    }

    /// Get screen dimensions
    pub fn get_dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Get current hints
    pub fn get_hints(&self) -> &[HintPoint] {
        &self.hints
    }
}

impl CompositorHandler for OverlayApp {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_factor: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        self.draw(qh);
    }

    fn surface_enter(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for OverlayApp {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

impl LayerShellHandler for OverlayApp {
    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer: &LayerSurface) {
        self.should_close = true;
    }

    fn configure(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        _layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        self.width = configure.new_size.0;
        self.height = configure.new_size.1;

        if self.width == 0 || self.height == 0 {
            self.width = 1920;
            self.height = 1080;
        }

        self.configured = true;

        if self.hints.is_empty() {
            self.hints = calculate_hints(
                self.width,
                self.height,
                &self.config.hint_chars,
                self.config.hint_size,
            );
        }

        self.draw(qh);
    }
}

impl ShmHandler for OverlayApp {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for OverlayApp {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

delegate_compositor!(OverlayApp);
delegate_output!(OverlayApp);
delegate_shm!(OverlayApp);
delegate_layer!(OverlayApp);
delegate_registry!(OverlayApp);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_hints() {
        let hints = calculate_hints(1920, 1080, "abcd", 20);
        assert_eq!(hints.len(), 16);
        assert_eq!(hints[0].label, "aa");
        assert_eq!(hints[1].label, "ab");
    }

    #[test]
    fn test_find_hint() {
        let hints = calculate_hints(1920, 1080, "ab", 20);
        let matches = find_hint_by_prefix(&hints, "a");
        assert_eq!(matches.len(), 2);

        let exact = find_hint_exact(&hints, "ab");
        assert!(exact.is_some());
        assert_eq!(exact.unwrap().label, "ab");
    }
}
