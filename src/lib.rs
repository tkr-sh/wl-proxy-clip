mod clipboard;
mod config;
mod fd;
mod filter;
mod image;
mod mime;
mod security;
mod server;
mod state;
mod transport;
mod wire;

pub(crate) type Res<T> = Result<T, String>;

pub use {config::Settings, server::run};
