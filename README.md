<h1 style="text-align: center" align="center">

`wl-proxy-clip`

</h1>


<p align="center">
    <a href="https://github.com/tkr-sh/wl-proxy-clip">
        <img
            src="https://img.shields.io/github/v/release/tkr-sh/wl-proxy-clip?colorA=363a4f&colorB=b7bdf8&style=for-the-badge&logo=github&logoColor=cad3f5"
            alt="github release"
        />
    </a>
    <a href="https://github.com/tkr-sh/wl-proxy-clip/actions">
        <img
            src="https://img.shields.io/github/actions/workflow/status/tkr-sh/wl-proxy-clip/ci.yml?branch=main&colorA=363a4f&colorB=a6da95&style=for-the-badge&logo=rust&logoColor=cad3f5"
            alt="ci status"
        />
    </a>
    <!--
    <a href="https://crates.io/crates/wl-proxy-clip">
        <img
            src="https://img.shields.io/crates/d/wl-proxy-clip.svg?colorA=363a4f&colorB=b7bdf8&style=for-the-badge&logo=rust&logoColor=cad3f5"
            alt="crates.io downloads"
        />
    </a>
    -->
    <a href="https://github.com/tkr-sh/wl-proxy-clip">
        <img
            src="https://img.shields.io/github/stars/tkr-sh/wl-proxy-clip?colorA=363a4f&colorB=eed49f&style=for-the-badge&logo=star"
            alt="stars"
        />
    </a>
</p>


`wl-proxy-clip` is a small Wayland proxy that gives a client its own clipboard.
It sits between a Wayland client and the host compositor.

```
client application --> wl-proxy-clip --> host compositor
```

By default nothing crosses the proxy:

- the client never learns what the host copied
- when the client copies, its selection stays there until an explicit `copy`
  command

## Commands

The control socket accepts one command per line:

| Command | Effect |
| --- | --- |
| `paste` | Lends the host clipboard to the client, including text and PNG/JPEG images. |
| `copy` | Reads the client selection and writes text or a PNG/JPEG image to the host clipboard. |

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

Clap is enabled by default. For a minimal build without it:

```sh
cargo build --release --no-default-features
```

Or with Nix:

```sh
nix build
```

## Usage

```text
wl-proxy-clip [OPTIONS] <name> <downstream-socket> <control-socket>
```

The default build includes the CLI options described below. A build made with
`--no-default-features` accepts only the three positional arguments.

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

## CLI options

The default build includes a Clap-based CLI. Every limit and timeout can be
overridden:

| Option | Default |
| --- | --- |
| `--max-text-bytes <BYTES>` | 1048576 |
| `--max-image-bytes <BYTES>` | 16777216 |
| `--max-image-pixels <PIXELS>` | 64000000 |
| `--paste-timeout <SECONDS>` | 15 |
| `--copy-timeout <SECONDS>` | 15 |
| `--image-copy-timeout <SECONDS>` | 30 |
| `--idle-poll-interval <MILLISECONDS>` | 200 |
| `--gpu` | disabled |

The text and image size limits, copy timeouts, and pixel limit apply to `copy`.
`paste-timeout` controls how long the host clipboard is lent to the client.

`--gpu` adds `zwp_linux_dmabuf_v1` to the global allowlist for GPU buffer
forwarding. A build made with `--no-default-features` uses the defaults above
and has no command-line options.

Run `wl-proxy-clip --help` for the generated help text.

## Behavior and limits

- Text and PNG/JPEG selections are supported in both directions. `paste`
  lends host data to the client; `copy` reads the client selection.
- The host clipboard is lent to the client after `paste`.
- Text `copy` has a 15 second timeout, a 1 MiB limit, and requires valid UTF-8.
  These are the defaults and can be changed with the CLI options above.
- Image `copy` has a 30 second timeout, a 16 MiB limit, and a 64 megapixel
  limit by default. PNG and JPEG headers are checked before the data is copied.
- For `copy`, image data is validated by header only and is not re-encoded.
  Host applications still decode it, so treat images from the client as
  untrusted input.
- For `paste`, image data is forwarded directly from the compositor and is not
  validated or re-encoded by the proxy. The client decodes it.
- GIF, WebP, and SVG are not supported. SVG is intentionally excluded.
- Drag and drop is disabled in both directions.
- Only an allowlist of globals is exposed to the client. Input methods, screen
  capture, layer shell, session lock, window management, output control,
  clipboard managers, data-control, and similar protocols are hidden.
  `zwp_linux_dmabuf_v1` is allowed only with `--gpu`.
- `wl_registry.bind` is checked as well. Binding an interface outside the
  allowlist, an unadvertised global, a mismatched interface, or a version above
  the advertised version closes the connection.

`wl-proxy-clip` is a clipboard isolation aid, not a complete sandbox. It
filters the Wayland objects it needs to understand, but it is not a substitute
for proper client isolation.
