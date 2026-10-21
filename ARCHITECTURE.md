# Architecture

This document explains how `wl-proxy-clip` works internally. It covers the
process model, connection lifecycle, Wayland filtering, clipboard commands,
file descriptor handling, image validation, and the boundaries of the security
model.

## Purpose

`wl-proxy-clip` is a userspace proxy between a Wayland client and a compositor:

```text
client application -> wl-proxy-clip -> host compositor
```

The proxy accepts a downstream Unix socket, connects to the host compositor,
and forwards Wayland messages in both directions. It only interprets the
small part of the protocol needed for clipboard control.

The default state is isolation:

- the client does not see the host clipboard selection;
- the host does not see the client selection;
- drag and drop in either direction is blocked;
- only an allowlist of Wayland globals is exposed.

Clipboard transfer happens only when a command is sent to the control socket.

## Entry points and process model

`src/main.rs` is intentionally small. It parses exactly three arguments:

```text
wl-proxy-clip <name> <downstream-socket> <control-socket>
```

It calls `wl_proxy_clip::run`, which is re-exported from `src/lib.rs`. Usage
errors exit with status 2. Startup errors exit with status 1. With the `clap`
feature, `main` parses the optional runtime settings and passes them to `run`;
without it, `Settings::default()` is used.

`src/lib.rs` declares the modules and the shared `Res<T>` type:

```rust
pub(crate) type Res<T> = Result<T, String>;
```

`src/server.rs` owns the process-level lifecycle:

1. Resolve the compositor socket from `$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY`.
   If `WAYLAND_DISPLAY` is unset, use `wayland-0`.
2. Bind the control socket and start one control thread.
3. Bind the downstream Wayland socket.
4. Accept downstream connections forever.
5. For each accepted client, connect to the compositor and start two worker
   threads.

The design uses blocking I/O and one thread per direction. There is no async
runtime.

## Per-connection state

Each accepted client gets one `Arc<Connection>` from `src/state.rs`.

```text
Connection
  settings: Settings
  state:    Mutex<ProxyState>
  commands: Mutex<VecDeque<Command>>
```

`Settings` carries the runtime limits and timing copied from the process
configuration: paste timeout, copy timeouts, idle poll interval, maximum text
and image sizes, maximum image dimensions, and the GPU flag.

`Command` is either `Paste` or `Copy`. Commands are queued by the control
thread and consumed by the compositor-to-client worker.

`ProxyState` contains the protocol bookkeeping for one Wayland connection:

- `objects: HashMap<u32, ObjectKind>` tracks object IDs that matter to the
  proxy.
- `globals: HashMap<u32, GlobalInfo>` stores advertised allowed globals with
  their interface and version.
- `mimes: HashMap<u32, Vec<String>>` stores advertised MIME types for data
  sources and data offers.
- `device: Option<u32>` is the current `wl_data_device` object.
- `host_offer: Option<u32>` is the current host clipboard offer. It is
  retained instead of being sent to the client.
- `client_source: Option<u32>` is the current client data source. It is
  retained instead of being sent to the compositor.
- `loan: Option<Loan>` records the host offer currently lent to the client by
  `paste`, together with its expiration time.

`ObjectKind` only tracks the objects the proxy needs to understand:

```text
Display   wl_display
Registry  wl_registry
Manager   wl_data_device_manager
Device    wl_data_device
Source    wl_data_source created by the client
Offer     wl_data_offer created by the compositor
```

Unknown objects and unknown opcodes are forwarded unchanged. This keeps the
proxy permissive for normal application traffic while special-casing clipboard
objects.

The global connection list is stored in `src/server.rs` as:

```rust
Arc<Mutex<Vec<Weak<Connection>>>>
```

It holds weak references so finished connections can be dropped. A control
command is applied to every connection that is still alive.

## Threads

There are three kinds of threads:

1. The main thread accepts new downstream connections.
2. The control thread reads commands from the control socket.
3. Two workers handle the two directions of each Wayland connection.

For every accepted client, both socket ends are cloned. One worker forwards
client requests to the compositor. The other forwards compositor events to
the client and injects clipboard events.

When a worker returns, it shuts down both sockets. This makes the opposite
worker exit as well. The connection is then removed from the weak list the
next time the control thread prunes it.

## Message and file descriptor transport

Wayland messages are read and written with `wlproxy::proto`. A Wayland
message may be accompanied by file descriptors sent through Unix socket
ancillary data (`SCM_RIGHTS`).

`wlproxy` only handles the byte stream. `src/transport.rs` wraps the streams:

- `FdReader` implements `Read`. It calls `recv_fds` and queues any received
  file descriptors in `pending_fds`.
- `FdWriter` implements `Write`. It attaches queued descriptors to the next
  write with `send_fds`.
- `send` sends the next forwarded message with all pending descriptors, then
  closes the proxy's copies of those descriptors.

This mirrors the way libwayland queues file descriptors: descriptors are
associated with the connection and delivered in order, not tied to a specific
message. A message that carries an fd must therefore never be dropped
silently, because that would desynchronize the fd queue.

The fd queue size per read is limited to 28 descriptors, matching common
Wayland client behavior.

## Client-to-compositor filtering

`transport::forward_requests` reads packets from the client, asks
`filter::request` for a verdict, and forwards only `Verdict::Forward` packets.

`filter::request` tracks and filters these requests:

- `wl_display.get_registry`: records the new registry object.
- `wl_registry.bind`: requires an allowed interface, an advertised global
  name, a matching interface, and a non-zero version. A version above the
  advertised version is rejected. Binding `wl_data_device_manager` registers
  the new object as `Manager`.
- `wl_data_device_manager.create_data_source`: records a `Source`.
- `wl_data_device_manager.get_data_device`: records the `Device`.
- `wl_data_source.offer`: appends the advertised MIME type to the source.
- `wl_data_device.start_drag`: dropped. Drag and drop from the client is not
  supported.
- `wl_data_device.set_selection`: stores the source ID in `client_source` and
  drops the request. The compositor never learns the client selection.
- `wl_data_offer.receive`: allowed only for the active loaned offer, before
  expiration, and only for text. Anything else closes the connection. Closing
  is deliberate because the request carries an fd and cannot be dropped
  safely.

All other requests are forwarded.

## Compositor-to-client filtering

`transport::forward_events` reads packets from the compositor, asks
`filter::event` for a verdict, and forwards only allowed packets.

`filter::event` handles these events:

- `wl_display.delete_id`: removes tracked object and MIME state.
- `wl_registry.global`: allowed globals are recorded and forwarded. Everything
  else is dropped, so the client never learns it exists.
- `wl_registry.global_remove`: removes the recorded global and is forwarded only
  if the global was previously known.
- `wl_data_device.data_offer`: records a new `Offer` and clears old MIME data
  for that object ID.
- `wl_data_offer.offer`: appends the advertised MIME type to the offer.
- `wl_data_device.enter`, `leave`, `motion`, `drop`: dropped. Drag and drop
  toward the client is not supported.
- `wl_data_device.selection`: stores the offer in `host_offer` and drops the
  event. The client does not learn what the host copied.

All other events are forwarded.

## Injection boundary

The compositor-to-client worker is the only worker that injects messages into
the client stream. It injects only when the compositor socket has no pending
input and no pending file descriptors:

```text
pending_bytes(compositor) == 0 && pending_fds.is_empty()
```

This condition is checked with `FIONREAD` and the local fd queue. It ensures
that injected file descriptors arrive after all file descriptors already in
flight from the compositor.

After injecting commands, the worker waits up to 200 ms for compositor input.
If nothing arrives, the loop continues, which gives queued commands and loan
expiration another chance to run while the connection is idle.

`clipboard::prepare` drains the command queue and runs under the state lock.
It returns a list of `Injection` values. Each injection contains a Wayland
packet and an optional owned file descriptor. `forward_events` writes each
injection directly to the client through an `FdWriter`.

## `paste`

`paste` lends the host clipboard to the client for 15 seconds by default.

The command requires:

- a `wl_data_device` object, so the proxy knows where to inject the selection;
- a `host_offer`, which is the host clipboard selection retained earlier;
- at least one text MIME type advertised by that offer.

If those conditions hold, `prepare`:

1. Stores `Loan { offer, until: now + settings.paste_timeout }`.
2. Injects `wl_data_device.selection(offer)` into the client.

The client can then request data from that offer. `filter::request` only
allows `wl_data_offer.receive` while the loan is active, for the same offer,
and for a text MIME type.

When the loan expires, `prepare` clears it and injects
`wl_data_device.selection(null)`, which removes the selection from the client.

Host to client `paste` is still text only. Image transfer is implemented only
for `copy`.

## `copy`

`copy` reads the client selection and writes it to the host clipboard.

The command requires a `client_source` retained by `set_selection`. The proxy
then selects a MIME type from the MIME list advertised by that source:

- image is preferred over text;
- supported images are PNG and JPEG;
- the nonstandard `image/jpg` alias is requested from the client but
  canonicalized to `image/jpeg` for the host;
- text falls back to the ordered `TEXT_MIMES` list.

The selected target is represented by `CopyTarget`:

```text
request    MIME type requested from the client
host       canonical MIME type written to the host clipboard
max_bytes  size limit for this kind of data
timeout    read timeout for this kind of data
```

The defaults are:

```text
text:  1 MiB, 15 second timeout
image: 16 MiB, 30 second timeout
```

They can be overridden when the binary is built with the `clap` feature.

`prepare` creates a pipe with `pipe2` and `O_CLOEXEC`, starts a reader thread,
and injects `wl_data_source.send(mime, write_end)` into the client. The client
writes the selection into the pipe.

The reader thread:

1. Waits for readable data with `poll` and a deadline.
2. Reads into a 64 KiB buffer until EOF.
3. Stops if the size limit is exceeded or the deadline expires.
4. Validates the data.
5. Hands the data and MIME type to `wl-clipboard-rs`, which publishes it through
   the compositor data-control protocol and serves paste requests in the background.

Text must be valid UTF-8. Images go through the header validation described
below. The proxy never decodes or re-encodes image data.

## MIME handling

`src/mime.rs` contains the MIME policy.

Text MIME types are checked case-insensitively:

```text
text/plain;charset=utf-8
UTF8_STRING
text/plain
TEXT
STRING
```

Image MIME entries have both a request name and a canonical name. This is why
`image/jpg` can be requested from a client that advertises it while the host
receives the standard `image/jpeg`.

For `copy`, image MIME types are checked before text MIME types. For `paste`,
only text MIME types are considered.

## Image validation

`src/image.rs` validates image headers without decoding the image.

PNG validation checks:

- the 8-byte PNG signature;
- the first chunk has length 13 and type `IHDR`;
- the width and height fields;
- the image is at most 64 megapixels;
- the data is at least large enough to contain the IHDR header.

JPEG validation checks:

- the `FF D8` start marker;
- marker segments until the start of scan;
- every start-of-frame marker found before the scan;
- width and height from the frame header;
- the image is at most 64 megapixels;
- segment lengths stay within the data.

The validation is a sanity check, not a full parser. It rejects truncated
headers, obvious malformed data, and images whose dimensions could create a
large decompression cost. Host applications still perform the real decode.

SVG, GIF, and WebP are intentionally not supported.

## Control socket

`src/server.rs` starts a control thread that accepts connections and reads one
command per line:

```text
paste
copy
```

Unknown commands are logged and stop processing that line. A valid command is
enqueued on every live connection.

There is no authentication on the control socket. Any process that can connect
to it can control all active connections. Socket permissions and the runtime
directory are therefore part of the security boundary.

## Global allowlist and security model

`src/config.rs` contains `ALLOWED_GLOBALS`. Only these globals are advertised
to the client. The list covers the core compositor, shared memory, seats,
outputs, XDG shell, decorations, output management, viewport, fractional
scaling, presentation, single-pixel buffers, cursor shapes, text input v3,
`wl_data_device_manager`, pointer constraints, relative pointers, and tearing
control. `zwp_linux_dmabuf_v1` is added when `Settings::should_support_gpu` is
enabled, which is exposed as `--gpu` in the optional CLI.

Everything else is hidden. This includes input methods, screen capture, layer
shell, session lock, foreign-toplevel and workspace management, output and
gamma control, DRM leases, virtual input, primary selection, data-control,
security contexts, activation, and idle notification protocols.

There are two enforcement layers:

1. `wl_registry.global` events outside the allowlist are dropped and not
   recorded in `ProxyState`.
2. `wl_registry.bind` requests are checked. The interface must be allowed, the
   global name must have been advertised, the interface must match, and the
   version must be non-zero and no higher than the advertised version.

Violations close the connection.

Drag and drop is refused in both directions. `wl_data_device.start_drag` is
dropped, as are the compositor enter, leave, motion, and drop events. Both
clipboard selections are still withheld by default. Clipboard data moves only
through explicit commands.

Important limits of this model:

- `wl-proxy-clip` is not a sandbox. It reduces clipboard and input exposure,
  but it does not isolate the client by itself.
- Image bytes are validated but not sanitized. A host application or clipboard
  manager may still decode malicious image data.
- The control socket is trusted local input. Protect it with filesystem
  permissions.
- The proxy logs MIME types and errors, but it does not log clipboard content.

## Failure handling

Errors are represented as strings through `Res<T>`. A filtering error, a
malformed filtered message, or an I/O error ends the corresponding worker.
The worker logs the error, shuts down both sockets, and lets the other worker
exit.

Recoverable accept and connection errors are logged and the process continues
accepting new clients. Clipboard read failures, timeouts, invalid data, and
`wl-clipboard-rs` failures are logged and do not terminate the Wayland
connection.

## Module map

- `src/main.rs`: argument parsing, optional Clap CLI, and process exit codes.
- `src/lib.rs`: module list and public `run` entry point.
- `src/server.rs`: listeners, control thread, connection workers.
- `src/state.rs`: object tracking, clipboard state, command queue.
- `src/filter.rs`: request and event filtering rules.
- `src/transport.rs`: packet forwarding and fd passing.
- `src/clipboard.rs`: `paste`, `copy`, pipe handling, host clipboard writing.
- `src/mime.rs`: supported text and image MIME types.
- `src/image.rs`: PNG and JPEG header validation.
- `src/wire.rs`: Wayland argument encoding and decoding helpers.
- `src/fd.rs`: pipe, poll, and pending-byte helpers.
- `src/config.rs`: runtime settings, default limits, allowed globals.

## Tests

Unit tests cover:

- disallowed and unadvertised global binding;
- tracking of `wl_data_device_manager`;
- refusal of unloaned host offer reads;
- allowed text reads during a loan;
- withholding the host selection event;
- allowlisted global advertisement and removal, including the GPU opt-in;
- MIME preference and JPEG alias canonicalization;
- PNG and JPEG header acceptance and rejection;
- dimension limits and malformed headers.

Run the full check with:

```sh
just check
```
