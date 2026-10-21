use {
    crate::{
        config::{READ_BUFFER_SIZE, Settings},
        fd::{pipe, readable},
        image,
        mime::{image_mime_for, is_paste_mime, text_mime_for},
        state::{Command as ClipboardCommand, Connection, Loan},
        wire::{string_body, uint_body},
    },
    std::{
        io::Read,
        os::fd::{AsRawFd, OwnedFd},
        thread,
        time::{Duration, Instant},
    },
    wl_clipboard_rs::copy::{MimeType, Options, Source},
    wlproxy::proto::Packet,
};

const DATA_DEVICE_SELECTION: u16 = 5;
const DATA_SOURCE_SEND: u16 = 1;

#[derive(Clone, Copy)]
struct CopyTarget {
    request:   &'static str,
    host:      &'static str,
    max_bytes: usize,
    timeout:   Duration,
}

impl CopyTarget {
    fn from_mimes(mimes: &[String], settings: Settings) -> Option<Self> {
        if let Some(image) = image_mime_for(mimes) {
            Some(Self {
                request:   image.request,
                host:      image.canonical,
                max_bytes: settings.max_image_bytes,
                timeout:   settings.image_copy_timeout,
            })
        } else {
            text_mime_for(mimes).map(|mime| {
                Self {
                    request:   mime,
                    host:      mime,
                    max_bytes: settings.max_text_bytes,
                    timeout:   settings.copy_timeout,
                }
            })
        }
    }
}

/// A message to inject into the client stream, prepared under the state lock.
pub(crate) struct Injection {
    pub(crate) packet: Packet,
    pub(crate) fd:     Option<OwnedFd>,
}

pub(crate) fn prepare(connection: &Connection) -> Vec<Injection> {
    let commands = connection.take_commands();
    let settings = connection.settings();
    let mut state = connection.state();
    let mut output = Vec::new();

    if let Some(device) = state.device &&
        state
            .loan
            .as_ref()
            .is_some_and(|loan| Instant::now() >= loan.until)
    {
        state.loan = None;
        output.push(Injection {
            packet: Packet {
                id:     device,
                opcode: DATA_DEVICE_SELECTION,
                body:   uint_body(0),
            },
            fd:     None,
        });
    }

    for command in commands {
        match command {
            ClipboardCommand::Paste => {
                let (Some(device), Some(offer)) = (state.device, state.host_offer) else {
                    eprintln!("paste: nothing to lend (did the client have focus?)");
                    continue;
                };
                if !state
                    .mimes
                    .get(&offer)
                    .is_some_and(|mimes| mimes.iter().any(|mime| is_paste_mime(mime)))
                {
                    eprintln!(
                        "paste: the host clipboard does not contain text or a supported image"
                    );
                    continue;
                }
                state.loan = Some(Loan {
                    offer,
                    until: Instant::now() + settings.paste_timeout,
                });
                // wl_data_device.selection(offer)
                output.push(Injection {
                    packet: Packet {
                        id:     device,
                        opcode: DATA_DEVICE_SELECTION,
                        body:   uint_body(offer),
                    },
                    fd:     None,
                });
            },
            ClipboardCommand::Copy => {
                let Some(source) = state.client_source else {
                    eprintln!("copy: the client has not copied anything");
                    continue;
                };
                let Some(target) = state
                    .mimes
                    .get(&source)
                    .and_then(|mimes| CopyTarget::from_mimes(mimes, settings))
                else {
                    eprintln!("copy: the client selection is neither text nor a supported image");
                    continue;
                };
                let (read_end, write_end) = match pipe() {
                    Ok(pipe) => pipe,
                    Err(error) => {
                        eprintln!("copy: failed to create pipe: {error}");
                        continue;
                    },
                };
                thread::spawn(move || {
                    read_into_host_clipboard(
                        read_end,
                        target.host,
                        target.max_bytes,
                        target.timeout,
                        settings.max_image_pixels,
                    );
                });
                // wl_data_source.send(mime, fd): the client writes into the pipe.
                output.push(Injection {
                    packet: Packet {
                        id:     source,
                        opcode: DATA_SOURCE_SEND,
                        body:   string_body(target.request),
                    },
                    fd:     Some(write_end),
                });
            },
        }
    }
    output
}

fn read_into_host_clipboard(
    read_end: OwnedFd,
    mime: &'static str,
    max_bytes: usize,
    timeout: Duration,
    max_image_pixels: u64,
) {
    let deadline = Instant::now() + timeout;
    let mut file = std::fs::File::from(read_end);
    let mut data = Vec::new();
    let mut buffer = [0; READ_BUFFER_SIZE];

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() || !readable(file.as_raw_fd(), remaining) {
            eprintln!("copy: the client did not respond in time");
            return;
        }

        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                if let Some(chunk) = buffer.get(..count) {
                    data.extend_from_slice(chunk);
                }
            },
            Err(error) => {
                eprintln!("copy: read failed: {error}");
                return;
            },
        }
        if data.len() > max_bytes {
            eprintln!("copy: selection exceeds {max_bytes} bytes, ignored");
            return;
        }
    }

    let valid = if mime.starts_with("image/") {
        image::validate(mime, &data, max_image_pixels)
    } else {
        std::str::from_utf8(&data).is_ok()
    };
    if !valid {
        eprintln!("copy: {mime} selection data is invalid, ignored");
        return;
    }

    let mut options = Options::new();
    options.omit_additional_text_mime_types(true);
    if let Err(error) = options.copy(
        Source::Bytes(data.into_boxed_slice()),
        MimeType::Specific(mime.to_owned()),
    ) {
        eprintln!("copy: failed to write the host clipboard: {error}");
    }
}

#[cfg(test)]
mod tests {
    use {super::*, crate::config::Settings};

    #[test]
    fn copy_target_uses_text_settings() {
        let settings = Settings {
            max_text_bytes: 42,
            copy_timeout: Duration::from_secs(7),
            ..Settings::default()
        };
        let mimes = vec!["text/plain".to_owned()];

        let target = CopyTarget::from_mimes(&mimes, settings).unwrap();

        assert_eq!(target.max_bytes, 42);
        assert_eq!(target.timeout, Duration::from_secs(7));
    }

    #[test]
    fn copy_target_uses_image_settings() {
        let settings = Settings {
            max_image_bytes: 84,
            image_copy_timeout: Duration::from_secs(9),
            ..Settings::default()
        };
        let mimes = vec!["image/png".to_owned()];

        let target = CopyTarget::from_mimes(&mimes, settings).unwrap();

        assert_eq!(target.max_bytes, 84);
        assert_eq!(target.timeout, Duration::from_secs(9));
    }
}
