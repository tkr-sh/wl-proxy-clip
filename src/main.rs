#[cfg(not(feature = "clap"))]
use std::path::Path;

#[cfg(feature = "clap")]
mod cli {
    use {
        clap::Parser,
        std::{path::PathBuf, time::Duration},
        wl_proxy_clip::Settings,
    };

    #[derive(Parser)]
    #[command(version, about)]
    struct Args {
        /// Name used in log messages.
        name:       String,
        /// Downstream Wayland socket.
        downstream: PathBuf,
        /// Control socket.
        control:    PathBuf,

        /// Maximum text selection size in bytes.
        #[arg(long, value_name = "BYTES", default_value_t = Settings::default().max_text_bytes)]
        max_text_bytes: usize,

        /// Maximum image selection size in bytes.
        #[arg(long, value_name = "BYTES", default_value_t = Settings::default().max_image_bytes)]
        max_image_bytes: usize,

        /// Maximum image dimensions in pixels.
        #[arg(long, value_name = "PIXELS", default_value_t = Settings::default().max_image_pixels)]
        max_image_pixels: u64,

        /// Paste timeout in seconds.
        #[arg(long, value_name = "SECONDS", default_value_t = Settings::default().paste_timeout.as_secs())]
        paste_timeout: u64,

        /// Text copy timeout in seconds.
        #[arg(long, value_name = "SECONDS", default_value_t = Settings::default().copy_timeout.as_secs())]
        copy_timeout: u64,

        /// Image copy timeout in seconds.
        #[arg(long, value_name = "SECONDS", default_value_t = Settings::default().image_copy_timeout.as_secs())]
        image_copy_timeout: u64,

        /// Event worker poll interval in milliseconds.
        #[arg(long, value_name = "MILLISECONDS", default_value_t = u64::try_from(Settings::default().idle_poll_interval.as_millis()).unwrap_or(u64::MAX))]
        idle_poll_interval: u64,

        /// Allow zwp_linux_dmabuf_v1 for GPU buffer forwarding.
        #[arg(long)]
        gpu: bool,
    }

    impl Args {
        const fn settings(&self) -> Settings {
            Settings {
                paste_timeout:      Duration::from_secs(self.paste_timeout),
                copy_timeout:       Duration::from_secs(self.copy_timeout),
                image_copy_timeout: Duration::from_secs(self.image_copy_timeout),
                idle_poll_interval: Duration::from_millis(self.idle_poll_interval),
                max_text_bytes:     self.max_text_bytes,
                max_image_bytes:    self.max_image_bytes,
                max_image_pixels:   self.max_image_pixels,
                should_support_gpu: self.gpu,
            }
        }
    }

    pub fn main() {
        let args = Args::parse();

        if let Err(error) =
            wl_proxy_clip::run(&args.name, &args.downstream, &args.control, args.settings())
        {
            eprintln!("wl-proxy-clip: {error}");
            std::process::exit(1);
        }
    }
}

#[cfg(feature = "clap")]
fn main() {
    cli::main();
}

#[cfg(not(feature = "clap"))]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [name, downstream, control] = args.as_slice() else {
        eprintln!("usage: wl-proxy-clip <name> <downstream-socket> <control-socket>");
        std::process::exit(2);
    };

    if let Err(error) = wl_proxy_clip::run(
        name,
        Path::new(downstream),
        Path::new(control),
        wl_proxy_clip::Settings::default(),
    ) {
        eprintln!("wl-proxy-clip: {error}");
        std::process::exit(1);
    }
}
