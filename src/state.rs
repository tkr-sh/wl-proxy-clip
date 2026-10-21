use std::{
    collections::{HashMap, VecDeque},
    sync::{Mutex, MutexGuard},
    time::Instant,
};

const DISPLAY_ID: u32 = 1;

/// The only objects tracked by the proxy. Opcodes come from wayland.xml.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ObjectKind {
    Display,
    Registry,
    Manager,
    Device,
    Source,
    Offer,
}

#[derive(Default)]
pub(crate) struct ProxyState {
    pub(crate) objects:       HashMap<u32, ObjectKind>,
    pub(crate) globals:       HashMap<u32, GlobalInfo>,
    pub(crate) mimes:         HashMap<u32, Vec<String>>,
    pub(crate) device:        Option<u32>,
    pub(crate) host_offer:    Option<u32>,
    pub(crate) client_source: Option<u32>,
    pub(crate) loan:          Option<Loan>,
}

pub(crate) struct Loan {
    pub(crate) offer: u32,
    pub(crate) until: Instant,
}

pub(crate) struct GlobalInfo {
    pub(crate) interface: String,
    pub(crate) version:   u32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Command {
    Paste,
    Copy,
}

pub(crate) struct Connection {
    state:    Mutex<ProxyState>,
    commands: Mutex<VecDeque<Command>>,
}

impl Connection {
    pub(crate) fn new() -> Self {
        let mut state = ProxyState::default();
        state.objects.insert(DISPLAY_ID, ObjectKind::Display);
        Self {
            state:    Mutex::new(state),
            commands: Mutex::new(VecDeque::new()),
        }
    }

    pub(crate) fn state(&self) -> MutexGuard<'_, ProxyState> {
        self.state.lock().expect("state lock poisoned")
    }

    pub(crate) fn enqueue(&self, command: Command) {
        self.commands
            .lock()
            .expect("command queue lock poisoned")
            .push_back(command);
    }

    pub(crate) fn take_commands(&self) -> VecDeque<Command> {
        let mut commands = self.commands.lock().expect("command queue lock poisoned");
        std::mem::take(&mut *commands)
    }
}
