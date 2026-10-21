use {
    crate::{
        Res,
        config::{Settings, is_allowed_global},
        mime::is_paste_mime,
        state::{GlobalInfo, ObjectKind, ProxyState},
        wire::{read_string, read_uint},
    },
    std::{io::Cursor, time::Instant},
    wlproxy::proto::{self, Packet},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Verdict {
    Forward,
    Drop,
}

/// Requests sent by the client to the compositor.
pub(crate) fn request(state: &mut ProxyState, packet: &Packet, settings: Settings) -> Res<Verdict> {
    let kind = state.objects.get(&packet.id).copied();
    Ok(match (kind, packet.opcode) {
        // wl_display.get_registry(new_id)
        (Some(ObjectKind::Display), 1) => {
            state
                .objects
                .insert(read_uint(&packet.body, 0)?, ObjectKind::Registry);
            Verdict::Forward
        },
        // wl_registry.bind(name, interface, version, new_id)
        (Some(ObjectKind::Registry), 0) => {
            let mut cursor = Cursor::new(packet.body.as_slice());

            let name = proto::read_arg_uint(&mut cursor)?;
            let interface = read_string(&mut cursor)?;
            let version = proto::read_arg_uint(&mut cursor)?;
            let id = proto::read_arg_uint(&mut cursor)?;

            if !is_allowed_global(&interface, settings.should_support_gpu) {
                return Err(format!(
                    "client attempted to bind disallowed global {interface}"
                ));
            }

            let Some(global) = state.globals.get(&name) else {
                return Err(format!(
                    "client attempted to bind unadvertised global {name} ({interface})"
                ));
            };
            if global.interface != interface {
                return Err(format!(
                    "client attempted to bind global {name} as {interface}, but it advertises {}",
                    global.interface
                ));
            }
            if version == 0 {
                return Err(format!(
                    "client attempted to bind {interface} with version 0"
                ));
            }

            let advertised_version = global.version;
            if version > advertised_version {
                return Err(format!(
                    "client requested {interface} version {version}, but only {advertised_version} is advertised"
                ));
            }

            if interface == "wl_data_device_manager" {
                state.objects.insert(id, ObjectKind::Manager);
            }
            Verdict::Forward
        },
        // wl_data_device_manager.create_data_source(new_id)
        (Some(ObjectKind::Manager), 0) => {
            state
                .objects
                .insert(read_uint(&packet.body, 0)?, ObjectKind::Source);
            Verdict::Forward
        },
        // wl_data_device_manager.get_data_device(new_id, seat)
        (Some(ObjectKind::Manager), 1) => {
            let id = read_uint(&packet.body, 0)?;
            state.objects.insert(id, ObjectKind::Device);
            state.device = Some(id);
            Verdict::Forward
        },
        // wl_data_source.offer(mime)
        (Some(ObjectKind::Source), 0) => {
            let mime = read_string(&mut Cursor::new(packet.body.as_slice()))?;
            state.mimes.entry(packet.id).or_default().push(mime);
            Verdict::Forward
        },
        // wl_data_device.start_drag: no drag and drop from the client.
        (Some(ObjectKind::Device), 0) => Verdict::Drop,
        // wl_data_device.set_selection(source, serial): keep the selection
        // for `copy` instead of forwarding it to the compositor.
        (Some(ObjectKind::Device), 1) => {
            let source = read_uint(&packet.body, 0)?;
            state.client_source = (source != 0).then_some(source);
            Verdict::Drop
        },
        // wl_data_offer.receive(mime, fd): only the active loan, while valid,
        // and only for supported text or image MIME types. Dropping a message
        // that carries an fd would desynchronize the fd queue, so terminate
        // the connection instead.
        (Some(ObjectKind::Offer), 1) => {
            let mime = read_string(&mut Cursor::new(packet.body.as_slice()))?;
            match state.loan {
                Some(ref loan)
                    if loan.offer == packet.id &&
                        Instant::now() < loan.until &&
                        is_paste_mime(&mime) =>
                {
                    Verdict::Forward
                },
                _ => {
                    return Err(format!("refused read from the host clipboard ({mime})"));
                },
            }
        },
        _ => Verdict::Forward,
    })
}

/// Events sent by the compositor to the client.
pub(crate) fn event(state: &mut ProxyState, packet: &Packet, settings: Settings) -> Res<Verdict> {
    let kind = state.objects.get(&packet.id).copied();
    Ok(match (kind, packet.opcode) {
        // wl_display.delete_id(id)
        (Some(ObjectKind::Display), 1) => {
            let id = read_uint(&packet.body, 0)?;
            state.objects.remove(&id);
            state.mimes.remove(&id);
            Verdict::Forward
        },
        // wl_registry.global(name, interface, version)
        (Some(ObjectKind::Registry), 0) => {
            let mut cursor = Cursor::new(packet.body.as_slice());
            let name = proto::read_arg_uint(&mut cursor)?;
            let interface = read_string(&mut cursor)?;
            let version = proto::read_arg_uint(&mut cursor)?;

            if is_allowed_global(&interface, settings.should_support_gpu) {
                state
                    .globals
                    .insert(name, GlobalInfo { interface, version });
                Verdict::Forward
            } else {
                Verdict::Drop
            }
        },
        // wl_registry.global_remove(name)
        (Some(ObjectKind::Registry), 1) => {
            let name = read_uint(&packet.body, 0)?;
            if state.globals.remove(&name).is_some() {
                Verdict::Forward
            } else {
                Verdict::Drop
            }
        },
        // wl_data_device.data_offer(new_id): the object must exist on the
        // client side, but without `selection` it has nothing to act on
        (Some(ObjectKind::Device), 0) => {
            let id = read_uint(&packet.body, 0)?;
            state.objects.insert(id, ObjectKind::Offer);
            state.mimes.remove(&id);
            Verdict::Forward
        },
        // wl_data_offer.offer(mime)
        (Some(ObjectKind::Offer), 0) => {
            let mime = read_string(&mut Cursor::new(packet.body.as_slice()))?;
            state.mimes.entry(packet.id).or_default().push(mime);
            Verdict::Forward
        },
        // enter / leave / motion / drop: no drag and drop toward the client.
        (Some(ObjectKind::Device), 1..=4) => Verdict::Drop,
        // wl_data_device.selection(offer): keep it for `paste`.
        (Some(ObjectKind::Device), 5) => {
            let offer = read_uint(&packet.body, 0)?;
            state.host_offer = (offer != 0).then_some(offer);
            Verdict::Drop
        },
        _ => Verdict::Forward,
    })
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::state::{GlobalInfo, Loan},
        std::time::Duration,
    };

    fn state_with(kind: ObjectKind, id: u32) -> ProxyState {
        let mut state = ProxyState::default();
        state.objects.insert(id, kind);
        state
    }

    fn advertise_global(state: &mut ProxyState, name: u32, interface: &str, version: u32) {
        state.globals.insert(
            name,
            GlobalInfo {
                interface: interface.to_owned(),
                version,
            },
        );
    }

    fn string_argument(value: &str) -> Vec<u8> {
        let mut body = Vec::new();
        proto::write_arg_string(&mut body, value).unwrap();
        body
    }

    fn bind_body(interface: &str, version: u32) -> Vec<u8> {
        let mut body = Vec::new();
        proto::write_arg_uint(&mut body, 1).unwrap();
        proto::write_arg_string(&mut body, interface).unwrap();
        proto::write_arg_uint(&mut body, version).unwrap();
        proto::write_arg_uint(&mut body, 2).unwrap();
        body
    }

    fn global_body(name: u32, interface: &str, version: u32) -> Vec<u8> {
        let mut body = Vec::new();
        proto::write_arg_uint(&mut body, name).unwrap();
        proto::write_arg_string(&mut body, interface).unwrap();
        proto::write_arg_uint(&mut body, version).unwrap();
        body
    }

    #[test]
    fn rejects_bind_of_disallowed_global() {
        let mut state = state_with(ObjectKind::Registry, 2);
        let packet = Packet {
            id:     2,
            opcode: 0,
            body:   bind_body("zwlr_screencopy_manager_v1", 1),
        };

        assert!(request(&mut state, &packet, Settings::default()).is_err());
    }

    #[test]
    fn rejects_bind_of_unadvertised_global() {
        let mut state = state_with(ObjectKind::Registry, 2);
        let packet = Packet {
            id:     2,
            opcode: 0,
            body:   bind_body("wl_compositor", 1),
        };

        assert!(request(&mut state, &packet, Settings::default()).is_err());
    }

    #[test]
    fn rejects_bind_with_mismatched_interface() {
        let mut state = state_with(ObjectKind::Registry, 2);
        advertise_global(&mut state, 1, "wl_compositor", 4);
        let packet = Packet {
            id:     2,
            opcode: 0,
            body:   bind_body("wl_shm", 1),
        };

        assert!(request(&mut state, &packet, Settings::default()).is_err());
    }

    #[test]
    fn tracks_data_device_manager_bind() {
        let mut state = state_with(ObjectKind::Registry, 2);
        advertise_global(&mut state, 1, "wl_data_device_manager", 3);
        let packet = Packet {
            id:     2,
            opcode: 0,
            body:   bind_body("wl_data_device_manager", 1),
        };

        assert_eq!(
            request(&mut state, &packet, Settings::default()).unwrap(),
            Verdict::Forward
        );
        assert_eq!(state.objects.get(&2), Some(&ObjectKind::Manager));
    }

    #[test]
    fn rejects_bind_with_version_above_advertised() {
        let mut state = state_with(ObjectKind::Registry, 2);
        advertise_global(&mut state, 1, "wl_compositor", 4);
        let packet = Packet {
            id:     2,
            opcode: 0,
            body:   bind_body("wl_compositor", 9),
        };

        assert!(request(&mut state, &packet, Settings::default()).is_err());
    }

    #[test]
    fn accepts_bind_with_version_below_advertised() {
        let mut state = state_with(ObjectKind::Registry, 2);
        advertise_global(&mut state, 1, "wl_compositor", 4);
        let packet = Packet {
            id:     2,
            opcode: 0,
            body:   bind_body("wl_compositor", 3),
        };

        assert_eq!(
            request(&mut state, &packet, Settings::default()).unwrap(),
            Verdict::Forward
        );
    }

    #[test]
    fn refuses_unloaned_host_offer_read() {
        let mut state = state_with(ObjectKind::Offer, 7);
        let packet = Packet {
            id:     7,
            opcode: 1,
            body:   string_argument("text/plain"),
        };

        assert!(request(&mut state, &packet, Settings::default()).is_err());
    }

    #[test]
    fn allows_text_read_during_loan() {
        let mut state = state_with(ObjectKind::Offer, 7);
        state.loan = Some(Loan {
            offer: 7,
            until: Instant::now() + Duration::from_secs(1),
        });
        let packet = Packet {
            id:     7,
            opcode: 1,
            body:   string_argument("text/plain"),
        };

        assert_eq!(
            request(&mut state, &packet, Settings::default()).unwrap(),
            Verdict::Forward
        );
    }

    #[test]
    fn allows_image_read_during_loan() {
        let mut state = state_with(ObjectKind::Offer, 7);
        state.loan = Some(Loan {
            offer: 7,
            until: Instant::now() + Duration::from_secs(1),
        });
        let packet = Packet {
            id:     7,
            opcode: 1,
            body:   string_argument("image/png"),
        };

        assert_eq!(
            request(&mut state, &packet, Settings::default()).unwrap(),
            Verdict::Forward
        );
    }

    #[test]
    fn refuses_unsupported_image_read_during_loan() {
        let mut state = state_with(ObjectKind::Offer, 7);
        state.loan = Some(Loan {
            offer: 7,
            until: Instant::now() + Duration::from_secs(1),
        });
        let packet = Packet {
            id:     7,
            opcode: 1,
            body:   string_argument("image/webp"),
        };

        assert!(request(&mut state, &packet, Settings::default()).is_err());
    }

    #[test]
    fn refuses_start_drag() {
        let mut state = state_with(ObjectKind::Device, 4);
        let packet = Packet {
            id:     4,
            opcode: 0,
            body:   Vec::new(),
        };

        assert_eq!(
            request(&mut state, &packet, Settings::default()).unwrap(),
            Verdict::Drop
        );
    }

    #[test]
    fn withholds_host_selection_event() {
        let mut state = state_with(ObjectKind::Device, 4);
        let packet = Packet {
            id:     4,
            opcode: 5,
            body:   9u32.to_ne_bytes().to_vec(),
        };

        assert_eq!(
            event(&mut state, &packet, Settings::default()).unwrap(),
            Verdict::Drop
        );
        assert_eq!(state.host_offer, Some(9));
    }

    #[test]
    fn advertises_allowed_global_event() {
        let mut state = state_with(ObjectKind::Registry, 2);
        let packet = Packet {
            id:     2,
            opcode: 0,
            body:   global_body(7, "wl_compositor", 6),
        };

        assert_eq!(
            event(&mut state, &packet, Settings::default()).unwrap(),
            Verdict::Forward
        );
        let global = state.globals.get(&7).unwrap();
        assert_eq!(global.interface, "wl_compositor");
        assert_eq!(global.version, 6);
    }

    #[test]
    fn hides_disallowed_global_event() {
        let mut state = state_with(ObjectKind::Registry, 2);
        let packet = Packet {
            id:     2,
            opcode: 0,
            body:   global_body(7, "zwlr_screencopy_manager_v1", 1),
        };

        assert_eq!(
            event(&mut state, &packet, Settings::default()).unwrap(),
            Verdict::Drop
        );
        assert!(!state.globals.contains_key(&7));
    }

    #[test]
    fn hides_gpu_global_by_default() {
        let mut state = state_with(ObjectKind::Registry, 2);
        let packet = Packet {
            id:     2,
            opcode: 0,
            body:   global_body(7, "zwp_linux_dmabuf_v1", 4),
        };

        assert_eq!(
            event(&mut state, &packet, Settings::default()).unwrap(),
            Verdict::Drop
        );
        assert!(!state.globals.contains_key(&7));
    }

    #[test]
    fn enables_gpu_global_with_setting() {
        let mut state = state_with(ObjectKind::Registry, 2);
        let packet = Packet {
            id:     2,
            opcode: 0,
            body:   global_body(7, "zwp_linux_dmabuf_v1", 4),
        };
        let settings = Settings {
            should_support_gpu: true,
            ..Settings::default()
        };

        assert_eq!(
            event(&mut state, &packet, settings).unwrap(),
            Verdict::Forward
        );
        assert!(state.globals.contains_key(&7));
    }

    #[test]
    fn binds_gpu_global_when_enabled() {
        let mut state = state_with(ObjectKind::Registry, 2);
        advertise_global(&mut state, 1, "zwp_linux_dmabuf_v1", 4);
        let packet = Packet {
            id:     2,
            opcode: 0,
            body:   bind_body("zwp_linux_dmabuf_v1", 1),
        };
        let settings = Settings {
            should_support_gpu: true,
            ..Settings::default()
        };

        assert_eq!(
            request(&mut state, &packet, settings).unwrap(),
            Verdict::Forward
        );
    }

    #[test]
    fn removes_advertised_global_event() {
        let mut state = state_with(ObjectKind::Registry, 2);
        advertise_global(&mut state, 7, "wl_compositor", 6);
        let packet = Packet {
            id:     2,
            opcode: 1,
            body:   7u32.to_ne_bytes().to_vec(),
        };

        assert_eq!(
            event(&mut state, &packet, Settings::default()).unwrap(),
            Verdict::Forward
        );
        assert!(!state.globals.contains_key(&7));
    }
}
