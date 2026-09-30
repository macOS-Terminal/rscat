//! rscat:能输出图片的 lolcat(Rust)。
//! 彩虹算法与 lolcat 100.0.1 逐字节一致;kitty/iTerm2 图片直通;
//! -e 在伪终端里跑命令(图片+彩虹兼得);-a 彩虹会话;-c 取消。

mod filter;
mod i18n;
mod image;
mod persist;
mod rainbow;
mod shell;
mod shell_detect;
#[cfg(unix)]
mod pty_unix;
#[cfg(windows)]
mod pty_windows;

use std::io::{IsTerminal, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::filter::LolcatFilter;
use crate::i18n::{Lang, Msg, help, t};
use crate::rainbow::{ColorMode, Painter};

#[cfg(unix)]
fn pty_supported() -> bool {
    true
}
#[cfg(windows)]
fn pty_supported() -> bool {
    pty_windows::supported()
}

#[cfg(unix)]
fn pty_run(cmd: &[String], env: &[(String, String)], comm: Option<&str>, flt: &mut LolcatFilter, stop: Option<&std::path::Path>, watch_parent: bool) -> i32 {
    pty_unix::run(cmd, env, comm, flt, stop, watch_parent)
}
#[cfg(windows)]
fn pty_run(cmd: &[String], env: &[(String, String)], _comm: Option<&str>, flt: &mut LolcatFilter, _stop: Option<&std::path::Path>, _watch: bool) -> i32 {
    let _ = env;
    pty_windows::run(cmd, flt, _stop)
}

/// macOS 需要在代理前以调用 shell 的名字重新 exec 自己一次(fastfetch 按父进程
/// 名判断终端/SHELL)。Linux 走 prctl、Windows 没有 pty 会话,两者都不需要做
/// 任何事,返回 true 让调用方继续正常流程。
///
/// 和 pty_run 一样按平台分发:该函数实现在 pty_unix 里,而 pty_unix 只在
/// `#[cfg(unix)]` 下编译,直接调用会让 Windows 目标编译失败。
#[cfg(unix)]
fn reexec_as(name: &str) -> bool {
    pty_unix::reexec_as(name)
}
#[cfg(windows)]
fn reexec_as(_name: &str) -> bool {
    true
}

struct Opts {
    spread: f64,
    freq: f64,
    seed: i64,
    animate: bool,
    duration: usize,
    speed: f64,
    invert: bool,
    truecolor: bool,
    force: bool,
    exec: Vec<String>,
    always: bool,
    cancel: bool,
    proto: String,
    quiet: bool,
    image_force: bool,
    init: Option<String>,
    lang_name: Option<String>,
    help: bool,
    version: bool,
    files: Vec<String>,
}

impl Opts {
    fn new() -> Opts {
        Opts {
            spread: 3.0,
            freq: 0.1,
            seed: 0,
            animate: false,
            duration: 12,
            speed: 20.0,
            invert: false,
            truecolor: false,
            force: false,
            exec: Vec::new(),
            always: false,
            cancel: false,
            proto: "auto".to_string(),
            quiet: false,
            image_force: false,
            init: None,
            lang_name: None,
            help: false,
            version: false,
            files: Vec::new(),
        }
    }
}

fn parse_number(lang: Lang, what: &str, s: &str) -> Result<f64, i32> {
    s.parse::<f64>().map_err(|_| {
        eprintln!("{} {what} = {s}", t(lang, Msg::ErrBadNumber));
        2
    })
}

/// 手写参数解析(保持帮助文本多语言,故不用 clap)。
/// -e/--exec 与 -- 之后均为 REMAINDER,直接作为待跑命令。
fn parse_args(lang: Lang, args: &[String]) -> Result<Opts, i32> {
    let mut o = Opts::new();
    let mut it = args.iter().peekable();
    while let Some(a) = it.next() {
        // --opt=value 形式
        let (name, eq_val): (&str, Option<&str>) = match a.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n, Some(v)),
            _ => (a.as_str(), None),
        };
        let mut val_of = |lang: Lang, what: &str| -> Result<String, i32> {
            if let Some(v) = eq_val {
                return Ok(v.to_string());
            }
            it.next().cloned().ok_or_else(|| {
                eprintln!("{}: {what}", t(lang, Msg::ErrMissingValue));
                2
            })
        };
        match name {
            "-p" | "--spread" => o.spread = parse_number(lang, "--spread", &val_of(lang, "--spread")?)?,
            "-F" | "--freq" => o.freq = parse_number(lang, "--freq", &val_of(lang, "--freq")?)?,
            "-S" | "--seed" => {
                let v = val_of(lang, "--seed")?;
                o.seed = v.parse::<i64>().map_err(|_| {
                    eprintln!("{} --seed = {v}", t(lang, Msg::ErrBadNumber));
                    2
                })?;
            }
            "--animate" => o.animate = true,
            "-d" | "--duration" => {
                let v = parse_number(lang, "--duration", &val_of(lang, "--duration")?)?;
                o.duration = v as usize;
            }
            "-s" | "--speed" => o.speed = parse_number(lang, "--speed", &val_of(lang, "--speed")?)?,
            "-i" | "--invert" => o.invert = true,
            "-t" | "--truecolor" => o.truecolor = true,
            "-f" | "--force" => o.force = true,
            "-e" | "--exec" => {
                let mut rest: Vec<String> = it.map(|s| s.to_string()).collect();
                if rest.first().map(|s| s.as_str()) == Some("--") {
                    rest.remove(0);
                }
                o.exec = rest;
                break;
            }
            "--" => {
                o.exec = it.map(|s| s.to_string()).collect();
                break;
            }
            "-a" | "--always" => o.always = true,
            "-c" | "--cancel" => o.cancel = true,
            "--proto" => {
                let v = val_of(lang, "--proto")?;
                if !["auto", "kitty", "iterm"].contains(&v.as_str()) {
                    eprintln!("{} --proto = {v}", t(lang, Msg::ErrBadNumber));
                    return Err(2);
                }
                o.proto = v;
            }
            "-q" | "--quiet" => o.quiet = true,
            "--image" => o.image_force = true,
            "--init" => o.init = Some(val_of(lang, "--init")?),
            "--lang" => o.lang_name = Some(val_of(lang, "--lang")?),
            "-h" | "--help" => o.help = true,
            "-V" | "--version" => o.version = true,
            _ if a.starts_with('-') && a.len() > 1 => {
                eprintln!("{}: {a}", t(lang, Msg::ErrUnknownFlag));
                return Err(2);
            }
            _ => o.files.push(a.to_string()),
        }
    }
    Ok(o)
}

fn random_seed() -> u64 {
    #[cfg(unix)]
    {
        if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
            let mut b = [0u8; 1];
            if f.read_exact(&mut b).is_ok() {
                return b[0] as u64;
            }
        }
    }
    // 兜底:纳秒时间(仅 Windows 或无 urandom 时)
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 % 256)
        .unwrap_or(42)
}

fn make_filter(o: &Opts, seed: u64, out: Box<dyn Write>) -> LolcatFilter {
    LolcatFilter::new(
        out,
        o.freq,
        o.spread,
        seed,
        Painter::new(ColorMode::detect(o.truecolor), o.invert),
        o.animate,
        o.duration,
        o.speed,
    )
}

/// 终端标题压栈(kitty/xterm 标题栈):进入 pty 模式前保存用户当前标题,
/// 否则 kitty 等会把标签页标题显示成前台进程名 "rscat"。
fn title_push() {
    if std::io::stdout().is_terminal() {
        let _ = std::io::stdout().write_all(b"\x1b[22;2t");
        let _ = std::io::stdout().flush();
    }
}

/// 退出 pty 模式后的终端状态还原:弹回标题、软复位(DECSTR,
/// 归零子进程残留的 SGR/字符集/滚动区域等)、字符集 ASCII、
/// 主字体、光标显示。session=true 时再多发 kitty 键盘协议清零
/// (内层会话 shell 若被 SIGKILL 强杀来不及弹栈,外层按键解析会残留)。
fn terminal_restore(session: bool) {
    if !std::io::stdout().is_terminal() {
        return;
    }
    let mut seq: Vec<u8> = Vec::new();
    seq.extend_from_slice(b"\x1b[23;2t\x1b[!p\x1b(B\x1b)B\x1b[10m\x1b[m\x1b[?25h");
    if session {
        seq.extend_from_slice(b"\x1b[>0u\x1b[?1;5;2004l");
    }
    let _ = std::io::stdout().write_all(&seq);
    let _ = std::io::stdout().flush();
}

/// 回退到 $SHELL//bin/sh 时只提示一次:-a/-e 里 pick_shell 会被调用两次,
/// 不加锁会重复刷屏。
static SHELL_FALLBACK_WARNED: AtomicBool = AtomicBool::new(false);

fn pick_shell(lang: Lang) -> shell_detect::Shell {
    match shell_detect::detect() {
        Some(s) => s,
        None => {
            if !SHELL_FALLBACK_WARNED.swap(true, Ordering::Relaxed) {
                eprintln!("{}", t(lang, Msg::ShellFallback));
            }
            shell_detect::fallback()
        }
    }
}

/// 中文 Windows 控制台默认代码页 936(GBK):ConPTY 按 GBK 解码我们的 UTF-8 输出,
/// 多字节字符被改写(▀→鈸?)、紧邻的 ESC 字节被吞,彩虹/图片序列随机损坏
/// (表现为"一个字一个色"的跳变与 logo 乱码)。切到 65001 (UTF-8) 后字节按原样解析。
#[cfg(windows)]
fn enable_utf8_console() {
    #[link(name = "kernel32")]
    extern "system" {
        fn SetConsoleOutputCP(wVersionID: u32) -> i32;
    }
    unsafe {
        SetConsoleOutputCP(65001);
    }
}

#[cfg(not(windows))]
fn enable_utf8_console() {}

fn main() {
    enable_utf8_console();
    // macOS:曾以 shell 名 re-exec 过自己的话,这里删掉那个临时可执行文件
    // (p_comm 在 exec 时已定型,删文件不影响它)。见 reexec_as(pty_unix 实现)。
    #[cfg(unix)]
    pty_unix::cleanup_after_reexec();
    let argv: Vec<String> = std::env::args().skip(1).collect();
    // 先用环境语言解析参数(--lang 本身需要在解析后才生效,错误信息用环境语言)
    let env_lang = Lang::detect();
    let o = match parse_args(env_lang, &argv) {
        Ok(o) => o,
        Err(c) => std::process::exit(c),
    };
    // --lang 覆盖
    let lang = match &o.lang_name {
        Some(n) => match Lang::parse(n) {
            Some(l) => l,
            None => {
                eprintln!("{}: {n}", t(env_lang, Msg::ErrUnknownLang));
                std::process::exit(2);
            }
        },
        None => env_lang,
    };

    if o.help {
        print!("{}", help(lang));
        return;
    }
    if o.version {
        println!("rscat {}", i18n::VERSION);
        return;
    }
    if let Some(sh) = &o.init {
        match shell::snippet(sh) {
            Some(s) => print!("{s}"),
            None => {
                eprintln!("{}", t(lang, Msg::ErrUnknownShell));
                std::process::exit(2);
            }
        }
        return;
    }
    if o.cancel {
        // -c:取消会话(会话内/外通用;会话主循环看到标记消失即收尾)
        persist::cleanup_stale();
        if persist::session_active() {
            persist::session_stop();
            println!("{}", t(lang, Msg::CancelOk));
        } else {
            println!("{}", t(lang, Msg::CancelNone));
        }
        return;
    }
    if o.always {
        // -a:彩虹会话
        if !o.exec.is_empty() {
            eprintln!("{}", t(lang, Msg::ErrConflictAlwaysExec));
            std::process::exit(2);
        }
        if persist::inside_session() {
            eprintln!("{}", t(lang, Msg::SessionNested));
            std::process::exit(1);
        }
        persist::cleanup_stale(); // 终端被关等异常退出留下的陈旧标记,属主已死则清除
        if persist::session_active() {
            eprintln!("{}", t(lang, Msg::SessionAlready));
            std::process::exit(1);
        }
        if !pty_supported() {
            eprintln!("{}", t(lang, Msg::ErrNeedUnixPty));
            std::process::exit(127);
        }
        // macOS:以调用 shell 的名字重新 exec 自己一次(fastfetch 按父进程名
        // 判断终端/SHELL,见 reexec_as)。必须赶在 session_start 与
        // title_push 之前 —— exec 保留 PID 但这两步有外部可见副作用,先做会
        // 在 exec 后重复执行一次。Linux 走 prctl,不需要 exec。
        {
            let sh = pick_shell(lang);
            if !reexec_as(&sh.name()) {
                eprintln!("{}", t(lang, Msg::ErrExecFailed));
                std::process::exit(127);
            }
        }
        if persist::session_start().is_err() {
            eprintln!("{} {:?}", t(lang, Msg::ErrReadFile), persist::session_marker());
            std::process::exit(1);
        }
        eprintln!("{}", t(lang, Msg::SessionEnter));
        std::env::set_var("RSCAT_SESSION", "1");
        let seed = if o.seed != 0 { o.seed as u64 } else { random_seed() };
        let out: Box<dyn Write> = Box::new(std::io::stdout().lock());
        let mut flt = make_filter(&o, seed, out);
        let sh = pick_shell(lang);
        let (sargv, senv) = shell_detect::session_argv(&sh);
        title_push();
        let rc = pty_run(&sargv, &senv, Some(&sh.name()), &mut flt, Some(&persist::session_marker()), true);
        flt.finish(std::io::stdout().is_terminal());
        terminal_restore(true);
        persist::session_clear_own();
        eprintln!("{}", t(lang, Msg::SessionExit));
        std::process::exit(rc);
    }
    if !o.exec.is_empty() {
        // -e:运行模式
        let seed = if o.seed != 0 { o.seed as u64 } else { random_seed() };
        let colorize = std::io::stdout().is_terminal() || o.force;
        if !colorize {
            // 与 lolcat 一致:非 tty 且未 -f 时原样直通(直接跑,不染色)
            let mut cmd = std::process::Command::new(&o.exec[0]);
            if o.exec.len() > 1 {
                cmd.args(&o.exec[1..]);
            }
            let rc = match cmd.status() {
                Ok(s) => s.code().unwrap_or(127),
                Err(e) => {
                    eprintln!("{} {}: {e}", t(lang, Msg::ErrExecFailed), o.exec[0]);
                    127
                }
            };
            std::process::exit(rc);
        }
        if !pty_supported() {
            eprintln!("{}", t(lang, Msg::ErrNeedUnixPty));
            std::process::exit(127);
        }
        // macOS:同 -a,先以调用 shell 的名字重新 exec 自己(见 reexec_as)。
        // 放在 title_push 之前,避免 exec 后重复推送一次标题。
        {
            let sh = pick_shell(lang);
            if !reexec_as(&sh.name()) {
                eprintln!("{}", t(lang, Msg::ErrExecFailed));
                std::process::exit(127);
            }
        }
        let out: Box<dyn Write> = Box::new(std::io::stdout().lock());
        let mut flt = make_filter(&o, seed, out);
        // 用真实调用 shell 包一层:fastfetch 等按父进程报 SHELL,
        // 直接 spawn 会显示 "rscat";包一层后显示 fish/zsh/bash 等本尊。
        let sh = pick_shell(lang);
        let eargv = shell_detect::exec_argv(&sh, &o.exec);
        let eenv = shell_detect::exec_env(&sh);
        title_push();
        let rc = pty_run(&eargv, &eenv, Some(&sh.name()), &mut flt, None, false);
        flt.finish(std::io::stdout().is_terminal());
        terminal_restore(false);
        std::process::exit(rc);
    }

    // ---- 过滤/图片模式 ----
    let colorize = std::io::stdout().is_terminal() || o.force;
    if !colorize {
        // 原样直通(与 lolcat/Python 版一致)
        let mut stdin = std::io::stdin().lock();
        let mut stdout = std::io::stdout().lock();
        let _ = std::io::copy(&mut stdin, &mut stdout);
        return;
    }
    let seed = if o.seed != 0 { o.seed as u64 } else { random_seed() };
    let out: Box<dyn Write> = Box::new(std::io::stdout().lock());
    let mut flt = make_filter(&o, seed, out);

    // 文件分类:图片 vs 文本
    let mut images: Vec<String> = Vec::new();
    let mut texts: Vec<String> = Vec::new();
    for path in &o.files {
        if o.image_force {
            images.push(path.clone());
            continue;
        }
        let head = std::fs::File::open(path)
            .ok()
            .and_then(|mut f| {
                let mut b = [0u8; 16];
                f.read_exact(&mut b).ok().map(|_| b.to_vec())
            })
            .unwrap_or_default();
        if image::sniff_image(&head) {
            images.push(path.clone());
        } else {
            texts.push(path.clone());
        }
    }

    let is_tty = std::io::stdout().is_terminal();
    if !images.is_empty() {
        // 图片字节经 flt.feed_raw 直通(同一把 stdout 锁,无死锁、保顺序)
        let proto = image::pick_proto(&o.proto).to_string();
        for path in &images {
            let data = match std::fs::read(path) {
                Ok(d) => d,
                Err(e) => {
                    eprintln!("{} {path}: {e}", t(lang, Msg::ErrReadFile));
                    continue;
                }
            };
            if proto == "kitty" {
                match image::as_png(&data) {
                    Ok((png, _, _)) => {
                        let mut buf = Vec::new();
                        image::send_kitty(&mut buf, &png);
                        flt.feed_raw(&buf);
                    }
                    Err(()) => {
                        eprintln!("{} {path}", t(lang, Msg::ErrNoDecode));
                        continue;
                    }
                }
            } else {
                let name = std::path::Path::new(path)
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.clone());
                let mut buf = Vec::new();
                image::send_iterm(&mut buf, &data, &name);
                flt.feed_raw(&buf);
            }
            if !o.quiet {
                flt.feed(format!("  {path}\n").as_bytes());
            }
        }
    }
    if !texts.is_empty() {
        for path in &texts {
            match std::fs::File::open(path) {
                Ok(mut f) => {
                    let mut buf = [0u8; 65536];
                    loop {
                        match f.read(&mut buf) {
                            Ok(0) => break,
                            Ok(n) => flt.feed(&buf[..n]),
                            Err(e) => {
                                eprintln!("{} {path}: {e}", t(lang, Msg::ErrReadFile));
                                break;
                            }
                        }
                    }
                }
                Err(e) => eprintln!("{} {path}: {e}", t(lang, Msg::ErrReadFile)),
            }
        }
    } else if images.is_empty() {
        let stdin = std::io::stdin();
        let mut lock = stdin.lock();
        let mut buf = [0u8; 65536];
        loop {
            match lock.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => flt.feed(&buf[..n]),
                Err(_) => break,
            }
        }
    }
    flt.finish(is_tty);
}
