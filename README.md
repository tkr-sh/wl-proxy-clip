<h1 style="text-align: center" align="center">

`wl-proxy-clip`

</h1>


`wl-proxy-clip` is a small Wayland proxy that gives a client its own clipboard.
It sits between a Wayland client and the host compositor.

```
client application --> wl-proxy-clip --> host compositor
```

By default nothing crosses the proxy:

- the client never learns what the host copied;
- when the client copies, its selection stays there until an explicit `copy`
  command.

The proxy supports text plus PNG and JPEG images, and only exposes the
clipboard on request.

## Commands

The control socket accepts one command per line:

| Command | Effect |
| --- | --- |
| `paste` | Lends the host clipboard to the client for 15 seconds, text only. |
| `copy` | Reads the client selection and writes it to the host clipboard. |

`copy` prefers a supported image when the client offers one, then falls back to
text. Commands apply to every active client connection.

## Requirements

- Linux with a Wayland compositor that supports `ext-data-control` or
  `wlr-data-control`
- Rust with edition 2024 support, or Nix

## Build

```sh
cargo build --release
```

Or with Nix:

```sh
nix build
```

The binary is `target/release/wl-proxy-clip` for Cargo, or
`result/bin/wl-proxy-clip` for Nix.

## Usage

```text
wl-proxy-clip <name> <downstream-socket> <control-socket>
```

- `<name>` is only used in log messages.
- `<downstream-socket>` is the Wayland socket that the client connects to.
- `<control-socket>` receives `paste` and `copy` commands.

The host compositor is found through `$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY`.
When `WAYLAND_DISPLAY` is unset, `wayland-0` is used.

Example:

```sh
export XDG_RUNTIME_DIR=/run/user/1000
wl-proxy-clip app1 \
  "$XDG_RUNTIME_DIR/wl-proxy-clip-wayland" \
  "$XDG_RUNTIME_DIR/wl-proxy-clip-control"
```

Point the client at `$XDG_RUNTIME_DIR/wl-proxy-clip-wayland` as its Wayland
socket. For a local client, that means setting `WAYLAND_DISPLAY` to
`wl-proxy-clip-wayland`. Then send commands to the control socket, for example
with `socat`:

```sh
printf 'paste\n' | socat - UNIX-CONNECT:"$XDG_RUNTIME_DIR/wl-proxy-clip-control"
printf 'copy\n'  | socat - UNIX-CONNECT:"$XDG_RUNTIME_DIR/wl-proxy-clip-control"
```

## Behavior and limits

- Text, PNG, and JPEG selections are supported for `copy`. Host to client
  `paste` remains text only.
- The host clipboard is lent to the client for 15 seconds after `paste`.
- Text `copy` has a 15 second timeout, a 1 MiB limit, and requires valid UTF-8.
- Image `copy` has a 30 second timeout, a 16 MiB limit, and a 64 megapixel
  limit. PNG and JPEG headers are checked before the data is copied.
- Image data is validated by header only and is not re-encoded. Host
  applications still decode it, so treat images from the client as untrusted
  input.
- GIF, WebP, and SVG are not supported. SVG is intentionally excluded.
- Drag and drop is disabled in both directions.
- Only an allowlist of globals is exposed to the client. Input methods, screen
  capture, layer shell, session lock, window management, output control,
  clipboard managers, data-control, and similar protocols are hidden.
- `wl_registry.bind` is checked as well. Binding an interface outside the
  allowlist, an unadvertised global, a mismatched interface, or a version above
  the advertised version closes the connection.

`wl-proxy-clip` is a clipboard isolation aid, not a complete sandbox. It
filters the Wayland objects it needs to understand, but it is not a substitute
for proper client isolation.
