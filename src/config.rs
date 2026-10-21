use std::time::Duration;

pub(crate) const PASTE_WINDOW: Duration = Duration::from_secs(15);
pub(crate) const COPY_TIMEOUT: Duration = Duration::from_secs(15);
pub(crate) const IMAGE_COPY_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const IDLE_POLL_INTERVAL: Duration = Duration::from_millis(200);
pub(crate) const MAX_TEXT_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_IMAGE_BYTES: usize = 16 * 1024 * 1024;
pub(crate) const MAX_IMAGE_PIXELS: u64 = 64_000_000;
pub(crate) const READ_BUFFER_SIZE: usize = 64 * 1024;

/// Globals exposed to the client. Everything else is hidden.
///
/// Add entries deliberately: each global is part of the client's attack surface.
pub(crate) const ALLOWED_GLOBALS: &[&str] = &[
    // Core
    "wl_compositor",
    "wl_subcompositor",
    "wl_shm",
    "wl_seat",
    "wl_output",
    // Windows
    "xdg_wm_base",
    "zxdg_decoration_manager_v1",
    "zxdg_output_manager_v1",
    // Scaling and rendering
    "wp_viewporter",
    "wp_fractional_scale_manager_v1",
    "wp_presentation",
    "wp_single_pixel_buffer_manager_v1",
    "wp_cursor_shape_manager_v1",
    // Text input as a normal client, not as an input method
    "zwp_text_input_manager_v3",
    // Clipboard, only through wl-proxy-clip's mediation
    "wl_data_device_manager",
    // Pointer lock, relative motion, tearing
    "zwp_pointer_constraints_v1",
    "zwp_relative_pointer_manager_v1",
    "wp_tearing_control_manager_v1",
    // Add "zwp_linux_dmabuf_v1" only when GPU buffer forwarding is enabled.
];
