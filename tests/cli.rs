//! 端到端 CLI 测试:直接跑编译好的二进制,覆盖 --version / -h / 过滤模式 /
//! 图片模式 / 非 tty 直通。和 src 里的单元测试互补 —— 那些测内部函数,
//! 这里测真实的进程行为(参数解析、stdout 字节、退出码)。

use std::io::Write;
use std::process::{Command, Stdio};

const VERSION: &str = concat!("rscat ", env!("CARGO_PKG_VERSION"), "-5");

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rscat"))
}

/// 剥掉所有 SGR 序列(`\e[...m`),便于对“彩虹化前的可见文本”做断言。
fn strip_sgr(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < data.len() {
        if data[i] == 0x1b && i + 1 < data.len() && data[i + 1] == b'[' {
            let mut j = i + 2;
            while j < data.len() && !(0x40..=0x7e).contains(&data[j]) {
                j += 1;
            }
            if j < data.len() {
                i = j + 1;
                continue;
            }
        }
        out.push(data[i]);
        i += 1;
    }
    out
}

/// 跑 `rscat <args>`,可选喂 stdin,返回 (退出码, stdout, stderr)。
fn run(args: &[&str], stdin: Option<&[u8]>) -> (i32, Vec<u8>, Vec<u8>) {
    let mut child = bin()
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn rscat");
    if let Some(data) = stdin {
        child.stdin.take().unwrap().write_all(data).unwrap();
    } else {
        drop(child.stdin.take());
    }
    let out = child.wait_with_output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        out.stdout,
        out.stderr,
    )
}

#[test]
fn version_prints_single_sourced_string() {
    let (code, stdout, _) = run(&["--version"], None);
    assert_eq!(code, 0);
    assert_eq!(String::from_utf8_lossy(&stdout).trim(), VERSION);
}

#[test]
fn help_substitutes_placeholder_and_parses_locale() {
    for lang in ["en", "zh-CN", "zh-TW", "ja"] {
        let (code, stdout, _) = run(&["--lang", lang, "-h"], None);
        assert_eq!(code, 0, "lang={lang}");
        let h = String::from_utf8_lossy(&stdout);
        assert!(h.starts_with("rscat "), "lang={lang}");
        assert!(h.contains(env!("CARGO_PKG_VERSION")), "lang={lang} must show version");
        assert!(!h.contains("{VERSION}"), "lang={lang} left a placeholder");
        assert!(h.contains("--truecolor"), "lang={lang} help must list options");
    }
}

#[test]
fn filter_mode_rainbows_stdin_when_forced() {
    let (code, stdout, _) = run(&["-f"], Some(b"hi\n"));
    assert_eq!(code, 0);
    assert!(
        stdout.windows(7).any(|w| w == b"\x1b[38;2;"),
        "forced color must emit truecolor SGR, got {:?}",
        String::from_utf8_lossy(&stdout)
    );
    assert!(stdout.contains(&b'h') && stdout.contains(&b'i'));
}

#[test]
fn non_tty_without_force_passes_through_verbatim() {
    // 与 lolcat 一致:非 tty 且未 -f 时原样直通,不加任何 SGR。
    let (code, stdout, _) = run(&[], Some(b"plain text\n"));
    assert_eq!(code, 0);
    assert_eq!(stdout, b"plain text\n");
}

#[test]
fn image_mode_emits_kitty_graphics_for_bmp() {
    // 2x2 24-bit BMP(sniff 认、image crate 能解码)
    let bmp: &[u8] = &[
        0x42, 0x4d, 0x46, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x36, 0x00, 0x00, 0x00, 0x28, 0x00,
        0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x01, 0x00, 0x18, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x10, 0x00, 0x00, 0x00, 0xc4, 0x0e, 0x00, 0x00, 0xc4, 0x0e, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x3c, 0x1e, 0xc8, 0x3c, 0x1e, 0xc8, 0x00, 0x00, 0x3c, 0x1e,
        0xc8, 0x3c, 0x1e, 0xc8, 0x00, 0x00,
    ];
    let path = std::env::temp_dir().join(format!("rscat-cli-test-{}.bmp", std::process::id()));
    std::fs::write(&path, bmp).unwrap();

    let (code, stdout, stderr) = run(
        &["-f", "--proto", "kitty", path.to_str().unwrap()],
        None,
    );
    let _ = std::fs::remove_file(&path);

    assert_eq!(code, 0, "stderr: {}", String::from_utf8_lossy(&stderr));
    assert!(
        stdout.windows(3).any(|w| w == b"\x1b_G"),
        "BMP must produce a kitty graphics APC, got {:?}",
        String::from_utf8_lossy(&stdout)
    );
    // 文件名行被逐字符彩虹化,故先剥掉 SGR 再找连续的路径。
    let visible = strip_sgr(&stdout);
    let p = path.to_str().unwrap().as_bytes();
    assert!(
        visible.windows(p.len()).any(|w| w == p),
        "image file name must appear (after stripping SGR), got {:?}",
        String::from_utf8_lossy(&visible)
    );
}
