# rscat 🌈🐱

[**English**](README.md) | [**简体中文**](README.zh-CN.md)

![](ass/nom.jpg)

## What?

`rscat` brings **image output to [lolcat](https://github.com/busyloop/lolcat)** — and
does it as a **cross-platform** tool.

lolcat rainbowizes text; it has no concept of pictures. Pipe **any** image-capable
tool into it — a system-info fetch, an image viewer, a plotter, a preview
utility — and the picture is destroyed: the graphics sequence may not survive the
pipe at all, and whatever does arrive gets painted over cell by cell. `rscat`
keeps the rainbow **and** the picture in the same stream, so anything that draws
becomes rainbow-safe.

- **Rainbow first.** The colouring is byte-for-byte compatible with lolcat 100.0.1 —
  same options, same output, drop-in replacement.
- **Image-aware.** Graphics protocols and picture cells pass through untouched.
- **Cross-platform.** Linux, macOS, FreeBSD and Windows from one Rust codebase —
  see the [platform matrix](#platform-support).

It is a Rust rewrite of an earlier single-file Python tool called `nyacat`.

## Why rscat exists

Rainbow colorizers treat a terminal stream as plain text. Anything that draws
*pixels* — kitty graphics, sixel, iTerm2 inline images, or block characters like
`█▀▄▌▐░▒▓` used to fake an image in text — is either mangled or colourised into
nonsense. That is fine for `cat`, and fatal for a logo.

`rscat` is built around the opposite assumption: it understands the escape stream
well enough to tell **pixels from text**, and colours only the text.

## Demo

The difference shows up the moment a tool emits both a picture and text. Anything
that draws to the terminal is fair game:

```bash
# system-info fetches with a logo
fastfetch | rscat
neofetch | rscat

# image viewers and previewers
chafa photo.png | rscat
viu artwork.jpg | rscat

# anything that plots or renders, including your own scripts
my-plot-script.py | rscat
```

Without rscat the picture is colourised into noise or lost entirely; with it the
picture survives and the text is rainbow.

Three ways to use it:

```bash
rscat logo.png          # image mode: draw a PNG/JPEG/GIF/BMP/WebP directly
some-command | rscat    # filter mode: rainbowize a stream, images intact
rscat -e fastfetch      # run mode: run a command in a pty, images + rainbow together
```

> **One caveat on pipes.** A tool that checks whether stdout is a terminal may
> skip image output when piped. fastfetch is the common case. Installing a package
> wires this up for you (see [Installation](#installation)); from source, `rscat --init`
> once and it is handled: the snippet defines a small `fastfetch` wrapper that
> appends `--pipe false` **only when the pipeline's reader really is `rscat`**:
>
> ```bash
> fastfetch | rscat        # images + rainbow, no flags needed
> fastfetch | lolcat       # untouched — that reader isn't rscat
> fastfetch > out.txt      # no image escapes written into the file
> fastfetch                # untouched (fastfetch already draws on a tty)
> ```
>
> The check is deliberately narrow: `--pipe false` is *not* added merely because
> stdout is a pipe (that would change the behaviour of `fastfetch | lolcat` or
> `fastfetch | grep`). The wrapper is defined for bash, zsh, sh and fish. Use
> `command fastfetch` to bypass it, or pass `--pipe` yourself and nothing is
> appended. If some other tool hides its images when piped and has no such flag,
> use `rscat -e <tool>` instead, which gives it a real pty.

## Features

| | |
|---|---|
| **lolcat-compatible rainbow** | Byte-for-byte identical to lolcat 100.0.1 (verified against 8 reference cases, truecolor and 256-color). Truecolor by default, so the gradient is smooth per character rather than banded. |
| **Image passthrough** | kitty graphics protocol / sixel / iTerm2 OSC sequences are forwarded verbatim, so images are never corrupted. |
| **Picture cells preserved** | Block characters carrying their own colour — how `fastfetch`, `chafa` and friends draw images in a text terminal — pass through untouched. Text, including ASCII-art logos, still gets the rainbow. |
| **Cross-platform** | One codebase, four platforms: Linux, macOS, FreeBSD, Windows. Native installers and packages for each. |
| **Run mode** | `rscat -e <command>` — run anything in a pty and get images *and* rainbow text, no wrapper needed, no per-tool flags. |
| **Image mode** | `rscat logo.png` — display a local image directly (PNG/JPEG/GIF/BMP/WebP). |
| **Rainbow session** | `rscat -a` makes every later command rainbow; `rscat -c` cancels. |
| **Multilingual** | English / 简体中文 / 繁體中文 / 日本語, follows system `LANG`, or `--lang`. |
| **Calling-shell detection** | `-e`/`-a` find the real shell in the parent chain, so a fetch tool's SHELL module shows fish/zsh/bash instead of `rscat`. |
| **Six shells** | Integration snippets for bash, zsh, sh, fish, PowerShell and cmd. |

## Platform support

| Platform | filter | image | `-e` run | `-a` session | Notes |
|---|---|---|---|---|---|
| Linux | ✅ tested | ✅ tested | ✅ tested | ✅ tested | primary development target |
| FreeBSD | ✅ expected | ✅ expected | ✅ expected | ✅ expected | packages provided (14.4 baseline, amd64/arm64), awaiting on-hardware verification |
| macOS | ✅ expected | ✅ expected | ✅ expected | ✅ expected | as above; `--proto iterm` for iTerm2/WezTerm |
| Windows | ✅ tested | ✅ tested (WezTerm + `--proto iterm`) | ❌ errors clearly | ❌ errors clearly | `-e`/`-a` need ConPTY, planned for a later release; rscat reports the limitation instead of emitting garbage |

"Expected" means the code only uses POSIX APIs common to those platforms (via
`libc`), but hasn't been verified on real hardware yet — issues welcome.

## Installation

Prebuilt binaries for every supported platform are published on the
[**Releases page**](../../releases). Download the file for your platform:

| Platform | Architecture | File |
|---|---|---|
| Linux | x86_64 | `rscat-1.1.6-5-x86_64.pkg.tar.zst` / `rscat_1.1.6-5_amd64.deb` / `rscat-1.1.6-5.x86_64.rpm` |
| Linux | aarch64 | `rscat-1.1.6-5-aarch64.pkg.tar.zst` / `rscat_1.1.6-5_arm64.deb` / `rscat-1.1.6-5.aarch64.rpm` |
| FreeBSD | amd64 | `rscat-1.1.6-5-freebsd-amd64.pkg` |
| FreeBSD | arm64 | `rscat-1.1.6-5-freebsd-arm64.pkg` |
| macOS | arm64 | `rscat-1.1.6-5-macos-aarch64.pkg` |
| macOS | x86_64 | `rscat-1.1.6-5-macos-x86_64.pkg` |
| Windows | x86_64 | `rscat-1.1.6-5-windows-x86_64-setup.exe` (installer) / `.msi` |
| Windows | aarch64 | `rscat-1.1.6-5-windows-aarch64.zip` (portable) |

### Linux

```bash
sudo pacman -U rscat-1.1.6-5-x86_64.pkg.tar.zst    # Arch / CachyOS
sudo apt install ./rscat_1.1.6-5_amd64.deb        # Debian / Ubuntu
sudo rpm -Uvh rscat-1.1.6-5.x86_64.rpm            # Fedora / openSUSE
```

### FreeBSD

```bash
pkg add ./rscat-1.1.6-5-freebsd-amd64.pkg
```

### macOS

```bash
sudo installer -pkg rscat-1.1.6-5-macos-aarch64.pkg -target /   # Apple Silicon
sudo installer -pkg rscat-1.1.6-5-macos-x86_64.pkg  -target /   # Intel
```

### Windows

Run the installer, or install the MSI from a terminal:

```powershell
.\rscat-1.1.6-5-windows-x86_64-setup.exe        # Inno Setup installer
msiexec /i rscat-1.1.6-5-windows-x86_64.msi     # WiX MSI
```

Both put `rscat` in `Program Files\rscat`, add it to your user `PATH` (removed
again on uninstall), and ship the shell snippets in `shells\`. The ARM64 zip is
portable — unzip it anywhere and run `rscat.exe`.

### Shell integration is wired automatically on Linux, FreeBSD and macOS

The Linux, FreeBSD and macOS packages run a post-install hook that connects the
snippets to your shells, so `fastfetch | rscat` works with images immediately —
no manual `--init` step:

| Target | Linux | FreeBSD | macOS | What it covers |
|---|:--:|:--:|:--:|---|
| `~/.bashrc` | ✅ | ✅ | ✅ | interactive **non-login** bash |
| `~/.bash_profile` | — | — | ✅ | macOS *login* bash (which never reads `~/.bashrc`) |
| `~/.zshrc` | ✅ | ✅ | ✅ | interactive zsh |
| `~/.profile` | ✅ | ✅ | ✅ | login `sh`, and login bash without a `~/.bash_profile` |
| `~/.shrc` | ✅ | ✅ | — | interactive **non-login** `sh` (Linux `sh` reads `$ENV`; macOS's does not) |
| `~/.config/fish/conf.d/rscat.fish` | ✅ | ✅ | ✅ | fish — auto-loaded, no rc edit |
| `/etc/profile.d/rscat.sh` | ✅ | ✅ | — | every account's login shells, including accounts created later |
| `/etc/zshenv` | — | — | ✅ | macOS system-wide, so `sudo -i`-style shells are covered too |
| fish vendor/site `conf.d` | ✅ | ✅ | ✅ | system-wide fish |

Rules it follows: it never overwrites your rc — it appends one fenced block
(`# >>> rscat shell integration >>>` … `# <<< … <<<`) that you can delete by hand,
keeping a `<rc>.rscat-bak` copy the first time; it is idempotent, so reinstalling
never stacks duplicates; and every step is non-fatal, so a hook failure can never
break a transaction. Uninstalling removes the block and the drop-in files again.

It also creates a missing rc (`~/.zshrc`, `~/.bash_profile`, `~/.profile`) when
that shell is the account's login shell, so a brand-new machine works too.

The snippets are sourced *by reference* (`[ -r /usr/share/rscat/shells/rscat.bash ] && . …`),
so upgrading the package updates the integration without touching your rc files.
(One exception: the fish `conf.d` snippet is a *copy*, not a reference.)

The Windows installers ship the snippets in `shells\` but do not wire rc files —
there, source the snippet yourself (next section).

### From source

```bash
cargo build --release
install -Dm755 target/release/rscat ~/.local/bin/rscat
```

Optional shell integration (idempotent — it sets up `PATH` and the `fastfetch`
wrapper described above):

```bash
rscat --init fish >> ~/.config/fish/config.fish   # fish
rscat --init bash >> ~/.bashrc                    # bash
rscat --init zsh  >> ~/.zshrc                     # zsh
rscat --init sh   >> ~/.shrc                      # sh  (interactive sh reads $ENV)
rscat --init powershell >> $PROFILE               # PowerShell
```


> **Note on build artifacts.** Published binaries and installers live on the
> [GitHub Releases](../../releases) page. The entire `build/` directory is
> git-ignored — packaging scripts, hand-off prompts, binaries and installers are
> all kept local only.

## Usage

```bash
# 1 run mode: run any command in a pty — images AND rainbow text
rscat -e fastfetch

# 2 image mode: display an image directly
rscat logo.png

# 3 rainbow session: everything after this is rainbow
rscat -a
rscat -c     # or type exit
```

Same options as lolcat (`-p/-F/-S/--animate/-d/-s/-i/-t/-f`) plus `-e/--exec`,
`-a/--always`, `-c/--cancel`, `--proto`, `--image`, `--init`, `--lang`.
Full multilingual help: `rscat -h`.

### Tools that hide their images when piped

Some tools check whether stdout is a terminal and skip image output when it isn't.
fastfetch is the common case — it needs `--pipe false`(has fixed)

```bash
fastfetch --pipe false | rscat
```

If a tool has no such flag, or you'd rather not think about it, use run mode
instead. `rscat -e` gives the command a real pty, so its own terminal detection
succeeds and it draws normally:

```bash
rscat -e chafa photo.png
rscat -e fastfetch
```

For a clean redirect to a file, bypass rscat entirely (`command fastfetch`, or
just run the tool without a pipe).

## How it works

`rscat` parses the escape stream instead of treating it as bytes. Two kinds of
thing are recognised as *not text* and are forwarded verbatim:

- **Graphics protocols** — kitty APC, sixel DCS, iTerm2 OSC 1337. These are
  emitted the instant the sequence completes, so a program waiting on a terminal
  reply never deadlocks.
- **Picture cells** — block characters (`█▀▄▌▐░▒▓` and friends) that carry their
  own colour. This is how tools like `fastfetch` and `chafa` render an image
  using nothing but text, one coloured cell per pixel. Each such cell is passed
  through with its original colour intact.

Everything else — including ASCII-art logos made of `/`, `#` or box-drawing
characters — is ordinary text and gets the rainbow. That distinction is what lets
any drawing tool show its picture *and* rainbow text in one pass, on any platform,
without knowing anything about that particular tool.

`-a` sessions add one more layer: the shell inside the session emits OSC 133
prompt marks, so the prompt and your own typed command line pass through
unrainbowed while command *output* is coloured. Terminal capability queries are
handled by the proxy itself where forwarding them would mislead the child (see
item 10 below), and the stream is forwarded unbuffered so a program waiting on a
terminal reply can never deadlock.

## Differences from nyacat (the Python predecessor)

1. **Fixed the image/text misalignment bug** in two layers. (a) termios: run mode
   used `setraw()` on both the child pty and the user terminal, clearing `ONLCR`
   so bare `\n` didn't return the carriage and output fell down the screen like a
   staircase; the child now stays cooked with only `ECHO` off. (b) protocol (the
   real culprit): `kitty-direct` transmits a *file path*, so fastfetch must query
   the cursor (`\x1b[6n`) to learn the image height — but it only waits ~50–100 ms,
   and a slightly slow proxied reply times out and the text lands below the image.
   The `kitty` type sends inline RGBA+zlib with deterministic cursor-up layout:
   zero queries, zero race.
2. **Fixed `--animate` always crashing** — the Python version called
   `bytearray.append(tuple)`, a guaranteed `TypeError`. Implemented properly in Rust.
3. **`-a` sessions / `-c` cancel are new**; `-e` replaces Python's `-c`.
4. **Non-PNG transcoding** moved from Pillow to the `image` crate (pure Rust, no
   system dependencies). A different resampler changes mean pixel values by ~2%
   (invisible); dimensions and protocol framing are identical.
5. **Calling-shell detection** — fastfetch reports SHELL from the parent process,
   so a direct spawn showed `rscat`. rscat now walks the kernel parent chain to
   find the real shell and wraps the command with it.
6. **zsh first-run wizard suppressed** — with no `~/.zshrc`, `zsh-newuser-install`
   would flood a session; a temporary `ZDOTDIR` with a minimal `.zshrc` prevents it.
7. **TER/TFO modules showing `rscat` or vanishing** — fastfetch's terminal module
   takes the first non-shell ancestor as the terminal, which was rscat itself.
   rscat now temporarily masquerades its `comm` as the calling shell so fastfetch
   reaches the real terminal.
8. **Terminal title and font state restored** on exit from `-e`/`-a`.
9. **`Enter` works in a `-a` session under fish.** The proxy used to read the real
   terminal with `ICRNL` still set, so the kernel rewrote the user's `CR` into
   `LF` before forwarding. bash/zsh accept either, but fish does its own line
   editing and treats `LF` as *insert a newline* — so Enter only ever wrapped.
   `stdin_cbreak()` now clears `ICRNL` too, letting the raw `CR` through; the
   child pty's own `ICRNL` still converts it for programs that aren't in raw mode.
10. **The proxy answers terminal capability queries itself.** fish 4.x asks
    whether the terminal supports the kitty keyboard protocol (`CSI ? u`). Passed
    through, a real kitty answers "yes" — but rscat forwards keystrokes verbatim
    and never re-encodes them as `CSI-u`, so fish then waited for key events that
    could never arrive. rscat now intercepts that one query and replies
    `CSI ? 0 u`. It deliberately does **not** touch XTVERSION or DECRQM, which the
    real terminal answers correctly and where a proxy reply would be wrong.
11. **Piped image output is wired up on install, and only where it should be.**
    The `fastfetch` wrapper appends `--pipe false` solely when the pipeline's
    reader is actually `rscat` — determined by matching the `pipe:[inode]` of the
    current stdout against each process's `fd/0` (and, for fish, by scanning the
    main process's children, since fish runs pipeline functions in-process with
    buffered output). An earlier attempt keyed on process groups, which is
    silently wrong under job control: shells put each pipeline in its own group,
    while `$$` inside a piped function still names the main shell.

## Development

```bash
cargo test          # 28 unit + 5 CLI tests: rainbow vectors, base64, PNG header,
                    # magic numbers, filter, PTY (termios + query interception),
                    # BMP/WebP decode, session markers, end-to-end CLI
cargo build --release
```

```
src/            main.rs (CLI) rainbow.rs filter.rs image.rs shell_detect.rs
                pty_unix.rs pty_windows.rs persist.rs i18n.rs shell.rs
                help_{en,zh_cn,zh_tw,ja}.txt
shells/         rscat.{bash,zsh,sh,fish,ps1,cmd} — the --init sources, and what
                the Linux/FreeBSD packages wire into your shells on install
tests/          cli.rs — end-to-end tests that run the built binary
build/          packaging recipes + per-platform hand-off prompts (git-ignored)
ass/            artwork
```

Platform-specific code is isolated: `pty_unix.rs` / `pty_windows.rs` and the
`cfg(unix)` / `cfg(windows)` pairs in `persist.rs`. Everything else — the filter,
the rainbow engine, the CLI — is shared, so a fix in the colour path benefits
every platform at once.

### Where the shell integration lives

Two layers, worth keeping in sync when you touch either:

| Layer | Files | What it does |
|---|---|---|
| Snippet *bodies* | `shells/rscat.*` | Define the `PATH` setup, the `fastfetch` wrapper, and (for the packaged case) the `.source`-able content. Compiled into the binary via `include_str!`, so `rscat --init <shell>` prints exactly these. |
| Wiring into users' shells | `build/linux/scripts/post-install`, `build/linux/scripts/pre-remove`, `build/macos/scripts/postinstall`, `build/rebuild-freebsd-packages.sh` | Run by the package manager to source those snippets from the right rc file for each shell, and to undo it on uninstall. |

Because the snippets are sourced *by reference*, editing a snippet file and
rebuilding the package is enough to update every user — no rc rewriting needed.

## Credits

The cat artwork (`ass/nom.jpg`) is from
[lolcat](https://github.com/busyloop/lolcat) by
[moe@busyloop.net](mailto:moe@busyloop.net) — the project this one is compatible
with and owes its rainbow to. The rainbow algorithm is a Rust port of lolcat's,
verified to produce byte-identical colour output.

## License

[MIT](LICENSE) © 2026 macOS Terminal. The colour code ported from lolcat and the
bundled cat artwork remain under lolcat's **BSD-3-Clause** license.

See [`THIRD-PARTY.md`](THIRD-PARTY.md) for that license text, the crates linked,
and what redistributors need to carry along.

Every package ships both files, so a binary install carries the notices BSD-3
requires: `/usr/share/licenses/rscat/` (Arch, RPM), `/usr/share/doc/rscat/`
(Debian) or `/usr/local/share/licenses/rscat/` (FreeBSD) and
`/usr/local/share/doc/rscat/` (macOS).
