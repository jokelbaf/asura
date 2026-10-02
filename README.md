# Asura

Asura is an HTTP(S) traffic inspector for Linux, built primarily for Windows applications running under Wine and Proton. It captures the traffic of the programs you choose without a system proxy, decrypts TLS with its own certificate authority, and shows every request in a native app.

## Features

- Per-program capture through eBPF: launch a program through Asura, attach to a running process, or capture every process.
- Steam and Proton support with the launch option `asura-run %command%`.
- TLS decryption, with the Asura certificate installed into Wine/Proton prefixes or the system trust store.
- Support of HTTP/1.1, HTTP/2, WebSocket and raw TLS/TCP streams.
- Body viewers for JSON, XML, HTML, JavaScript, CSS, images, forms, multipart, server-sent events, protobuf and gRPC.
- Filtering with a small query language (`host:example.com status:4xx -is:tunnel`).
- Tampering: auto-responder rules, breakpoints to edit requests and responses, replay, and a request composer.
- Save sessions as `.asura` files, and import or export HAR.

## How it works

eBPF programs attached to the root cgroup redirect the `connect()` calls of the selected process trees to a local proxy. A small helper loads them through pkexec and sends the file descriptors to the unprivileged app. The proxy relays bytes untouched and observes them, and switches a connection to an active engine only when a tampering rule could apply to it.

## Requirements

- Linux 6.5 or newer with cgroup v2.
- A systemd user session and polkit with an authentication agent.
- GTK 4.22+, libadwaita 1.9+ and GtkSourceView 5.18+.
- To build: rustup with a nightly toolchain and `rust-src`, plus `bpf-linker`.

## Building and installing

On Arch Linux:

```sh
cd packaging/arch
makepkg -si
```

Elsewhere:

```sh
rustup toolchain install nightly --component rust-src
cargo install bpf-linker
cargo build --release
sudo sh packaging/install.sh
sudo update-mime-database /usr/share/mime
```

The install prefix must always be `/usr`, because the polkit policy refers to the helper at a fixed path.

## Usage

1. Start Asura and choose what to capture: Launch Program, Attach to Process, or Capture All Processes in the Targets menu. For Steam games, set the launch options to `asura-run %command%`. From a terminal, run `asura-run -- command args...`.
2. Make the application trust the Asura certificate. `asura-run` installs it into the Wine prefix it detects. Otherwise use **Preferences** > **Wine Prefixes**, or **Preferences** > **Decryption** > **System Trust Store** for native Linux programs.
3. Press the start button (F12) to capture. Programs launched through Asura start capturing automatically.

Clients that reject the certificate are passed through without decryption, and are listed under Preferences > Decryption.

## License

This project is licensed under GPL-3.0. See [LICENSE](LICENSE) for details.
