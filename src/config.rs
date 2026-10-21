use std::time::Duration;

const DEFAULT_PASTE_TIMEOUT: Duration = Duration::from_secs(15);
const DEFAULT_COPY_TIMEOUT: Duration = Duration::from_secs(15);
const DEFAULT_IMAGE_COPY_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_IDLE_POLL_INTERVAL: Duration = Duration::from_millis(200);
const DEFAULT_MAX_TEXT_BYTES: usize = 1024 * 1024;
const DEFAULT_MAX_IMAGE_BYTES: usize = 16 * 1024 * 1024;
const DEFAULT_MAX_IMAGE_PIXELS: u64 = 64_000_000;

pub(crate) const READ_BUFFER_SIZE: usize = 64 * 1024;

#[derive(Clone, Copy, Debug)]
pub struct Settings {
    pub paste_timeout:      Duration,
    pub copy_timeout:       Duration,
    pub image_copy_timeout: Duration,
    pub idle_poll_interval: Duration,
    pub max_text_bytes:     usize,
    pub max_image_bytes:    usize,
    pub max_image_pixels:   u64,
    pub should_support_gpu: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            paste_timeout:      DEFAULT_PASTE_TIMEOUT,
            copy_timeout:       DEFAULT_COPY_TIMEOUT,
            image_copy_timeout: DEFAULT_IMAGE_COPY_TIMEOUT,
            idle_poll_interval: DEFAULT_IDLE_POLL_INTERVAL,
            max_text_bytes:     DEFAULT_MAX_TEXT_BYTES,
            max_image_bytes:    DEFAULT_MAX_IMAGE_BYTES,
            max_image_pixels:   DEFAULT_MAX_IMAGE_PIXELS,
            should_support_gpu: false,
        }
    }
}

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
];

pub(crate) fn is_allowed_global(interface: &str, should_support_gpu: bool) -> bool {
    ALLOWED_GLOBALS.contains(&interface) ||
        (should_support_gpu && interface == "zwp_linux_dmabuf_v1")
}
