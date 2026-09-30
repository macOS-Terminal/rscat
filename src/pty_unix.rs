//! 运行模式(Unix):在伪终端里跑子命令,stdout 彩虹转发,stdin 原样透传。
//! 关键修正(相对旧 Python 版曾经的 bug):
//!   子伪终端保持内核默认 cooked 模式(ONLCR 等输出加工必须保留,
//!   否则子进程写的裸 \n 不会被翻译成 \r\n,图片排版直接散架),
//!   只按模式处理 ECHO:-e 关(避免代理注入的查询应答被回显),
//!   -a 会话保留(readline 依赖 ECHO 标志决定是否自己回显用户输入);
//!   用户真终端只关 ICANON/ECHO(字节级输入,查询应答直达),
//!   IFLAG/OFLAG 原样保留(OPOST/ONLCR 必须活着)。
//! portable recipe:posix_openpt+grantpt+unlockpt+ptsname+fork+setsid,
//! Linux / FreeBSD / macOS 通用。

use std::ffi::CString;
use std::io::Write;
use std::os::unix::io::RawFd;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::filter::LolcatFilter;

static GOT_SIGINT: AtomicBool = AtomicBool::new(false);
static GOT_SIGWINCH: AtomicBool = AtomicBool::new(false);

extern "C" fn on_sigint(_: libc::c_int) {
    GOT_SIGINT.store(true, Ordering::Relaxed);
}
extern "C" fn on_sigwinch(_: libc::c_int) {
    GOT_SIGWINCH.store(true, Ordering::Relaxed);
}

fn copy_winsize(dst: RawFd) {
    unsafe {
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) != 0 {
            return;
        }
        if ws.ws_row == 0 && ws.ws_col == 0 && ws.ws_xpixel == 0 && ws.ws_ypixel == 0 {
            return;
        }
        libc::ioctl(dst, libc::TIOCSWINSZ, &ws);
    }
}

/// 子伪终端:cooked 保留 + 去回显(勿 setraw,会杀 ONLCR 致排版散架)。
///
/// -e 运行模式(interactive=false):关 ECHO。查询代理把真终端的应答原样
/// 写进子 pty,ECHO 开着的话子端会把应答回显成 `^[[?62;c` 之类乱码,
/// 甚至被 shell 当命令执行。
///
/// -a 会话模式(interactive=true):保留 ECHO。交互式 shell 的 readline 在
/// 启动时按 pty 的 ECHO 标志决定「内核是否已经回显输入」并缓存该状态
/// (readline 内部 _rl_echoing_p):ECHO 关掉时内核不回显,而 readline 也
/// 认为无需自己回显,结果用户敲的命令行完全不显示(回车后只剩命令输出)。
/// readline 在读终端查询应答期间会自行关闭回显,故会话模式下代理应答
/// 不会被回显。
/// 会话模式是否应保留子 pty 的回显。抽成纯函数以便单测:
/// -a 会话(watch_parent=true)保留(readline 靠它回显用户输入),
/// -e 运行模式关闭(避免代理应答被回显)。
fn keep_slave_echo(watch_parent: bool) -> bool {
    watch_parent
}

fn slave_termios(sfd: RawFd, interactive: bool) {
    unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(sfd, &mut t) != 0 {
            return;
        }
        if interactive {
            t.c_lflag |= libc::ECHO;
        } else {
            t.c_lflag &= !libc::ECHO;
        }
        libc::tcsetattr(sfd, libc::TCSANOW, &t);
    }
}

/// 用户真终端:字节级输入但保留 OPOST/ONLCR(勿 setraw)。
/// 返回旧 termios 供恢复;非终端返回 None。
///
/// 注意必须同时清 ICRNL:否则终端驱动把回车 CR(0x0d) 改写成 LF(0x0a),
/// 我们转发的是 LF。bash/zsh 的 readline 把 LF 也当提交,所以看不出问题;
/// 但 **fish 自己关掉 ICANON 做行编辑,只认 CR=执行、LF=插入换行** —— 于是
/// 出现「回车只换行、命令不执行」。清掉 ICRNL 后原始终端按下的 CR 原样
/// 传给子 shell,fish/bash/zsh 都能正确执行;子 pty 从端自身的 ICRNL 仍会
/// 把 CR 转成 NL 交给没开 raw 模式的程序,行为不变。
fn stdin_cbreak() -> Option<libc::termios> {
    unsafe {
        let mut saved: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(libc::STDIN_FILENO, &mut saved) != 0 {
            return None;
        }
        let mut t = saved;
        t.c_lflag &= !(libc::ICANON | libc::ECHO);
        t.c_iflag &= !libc::ICRNL;
        t.c_cc[libc::VMIN] = 1;
        t.c_cc[libc::VTIME] = 0;
        if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSADRAIN, &t) != 0 {
            return None;
        }
        Some(saved)
    }
}

fn restore_stdin(saved: &libc::termios) {
    unsafe {
        libc::tcsetattr(libc::STDIN_FILENO, libc::TCSADRAIN, saved);
    }
}

/// 会话代理必须**代答**的终端能力查询:kitty 键盘协议(`CSI ? u`)。
///
/// 为什么必须拦:rscat 不是终端模拟器 —— 它把用户的按键**原样**转发给子
/// pty,不会按 kitty 键盘协议把按键重编码成 `CSI 13 u` 之类。若把这条查询
/// 放行给真终端,真 kitty 会答"支持"(实测回 `CSI ? 0 u`),子 shell(fish 4.x
/// 启动就会问)于是启用该协议并等待 CSI-u 编码的按键,而它实际收到的永远是
/// 裸字节 —— 回车被当成普通字符插入,表现为「回车只换行、命令不执行」。
/// bash 不查询该协议,所以只有 fish 会话中招。
///
/// 处理:截掉查询,就地以「不支持」应答,查询不上行。
///
/// 注意**不要**顺手拦 `CSI > Ps q`(XTVERSION,问终端名)和 DECRQM:那些真
/// 终端能正确回答,代答反而会给出错误信息(fish 会据此错配终端特性)。
const KBD_QUERY: &[u8] = b"\x1b[?u";
const KBD_ANSWER: &[u8] = b"\x1b[?0u";

/// 过滤子进程输出里的 kitty 键盘查询并就地代答。
///
/// `pending` 跨批次残留:查询可能被 read() 从中间切断,尾部若是它的前缀就
/// 先扣住;若下一批没续上(循环空转),调用方会把 pending 当普通输出放行,
/// 避免把孤立 ESC 永久卡住。
fn kbd_query_filter(
    data: &[u8],
    pending: &mut Vec<u8>,
    mfd: RawFd,
    flt: &mut LolcatFilter,
) {
    let mut all = std::mem::take(pending);
    all.extend_from_slice(data);
    let mut plain: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < all.len() {
        let rest = &all[i..];
        if rest.starts_with(KBD_QUERY) {
            if !plain.is_empty() {
                flt.feed(&plain);
                plain.clear();
            }
            write_all(mfd, KBD_ANSWER); // 代真终端回答:不支持
            i += KBD_QUERY.len();
            continue;
        }
        // 可能被切断的查询前缀:扣住等续包。
        if rest.len() < KBD_QUERY.len() && KBD_QUERY.starts_with(rest) {
            *pending = rest.to_vec();
            if !plain.is_empty() {
                flt.feed(&plain);
            }
            return;
        }
        plain.push(all[i]);
        i += 1;
    }
    if !plain.is_empty() {
        flt.feed(&plain);
    }
}

/// 把 fd 设为非阻塞(O_NONBLOCK)。用于「父进程 stdin 不是终端」的场景:
/// 此时无法走 stdin_cbreak(它只对 tty 生效),但代理仍需把 stdin 转给子 pty,
/// 否则会话会收不到任何输入、按回车只换行不执行。非阻塞让 select 循环可以
/// 轮询 stdin 而不被阻塞。返回设置前是否已是阻塞态,供还原用。
fn set_nonblocking(fd: RawFd) -> bool {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL, 0);
        if flags < 0 {
            return false;
        }
        let was_blocking = flags & libc::O_NONBLOCK == 0;
        libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        was_blocking
    }
}

/// 还原 fd 的阻塞标志(clear O_NONBLOCK)。仅当 set_nonblocking 改过才需要,
/// 进程退出前清掉,避免把调用方(若有下游)的 stdin 留在非阻塞态。
fn clear_nonblocking(fd: RawFd) {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL, 0);
        if flags >= 0 {
            libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK);
        }
    }
}

fn write_all(fd: RawFd, mut data: &[u8]) {
    while !data.is_empty() {
        let n = unsafe { libc::write(fd, data.as_ptr() as *const libc::c_void, data.len()) };
        if n <= 0 {
            break;
        }
        data = &data[n as usize..];
    }
}

/// 改进程 comm。代理期间把 rscat 的进程名改成调用 shell 的名字,
/// 让 fastfetch 的 shell/终端模块(按父进程链找)显示本尊。
///
/// 为什么需要:fastfetch 的 terminalshell 检测沿父进程链向上走,遇到"已知
/// shell 名"就跳过。rscat 若保持自己的名字,会被当成链上第一个非 shell,
/// 于是 TER 显示 "rscat";伪装成 shell 后 fastfetch 会穿过 rscat 命中真正的
/// shell(p_comm 就是调用它的 zsh/fish)与更上面的真终端模拟器。
///
/// 实现按平台分两路,效果等价:
///   * Linux:`prctl(PR_SET_NAME)`,直接改 task comm,可逆、零成本。
///   * macOS:`prctl` 不存在,且 p_comm 取自**可执行文件的 vnode 名字**
///     (不是 argv[0] —— 实测 execve 传自定义 argv[0] 无效)。所以走
///     re-exec:在临时目录放一个以 shell 命名的链接/副本,带着"我只做
///     伪装"的标记再 exec 自己一次,由新进程继续跑代理。见 `reexec_as`.
#[cfg(target_os = "linux")]
fn set_comm(name: &str) {
    let mut buf = [0u8; 16];
    let bytes = name.as_bytes();
    let n = bytes.len().min(15);
    buf[..n].copy_from_slice(&bytes[..n]);
    unsafe {
        libc::prctl(libc::PR_SET_NAME, buf.as_ptr(), 0, 0, 0);
    }
}

#[cfg(not(target_os = "linux"))]
fn set_comm(_name: &str) {}

/// macOS:让本进程以 `name`(调用 shell 的名字)出现在系统进程表里。
///
/// 为什么不能用 set_comm 直接改:macOS 没有 prctl(PR_SET_NAME),而 p_comm
/// 取自**可执行文件的 vnode 名字**——实测 execve 传自定义 argv[0] 完全无效
/// (新进程的 p_comm 仍是该路径的 basename)。唯一可靠的办法就是"用一个
/// shell 名字的可执行文件重新 exec 自己一次"。
///
/// 做法:
///   1. 在临时目录建一个名为 <shell> 的硬链接(跨卷时退化为复制);
///   2. 以该路径 execv 自己,并设置 `RSCAT_COMM_AS` 标记;
///   3. exec 成功后 PID 不变、p_comm 已成为 shell 名。新进程随即删除临时
///      文件与目录 —— 实测 unlink 之后 p_comm 依然保留,故不留任何垃圾;
///   4. 新进程看到标记即跳过本函数,继续正常跑代理。
///
/// 与 Linux 的 prctl 路线效果等价(实测 SHE 报真实 shell 及版本、TER 穿透
/// 到真终端),都不改变用户可见的命令行为。
/// 返回 true 表示"已重新 exec 过"(调用方继续正常流程即可);exec 成功不返回。
#[cfg(target_os = "macos")]
pub fn reexec_as(name: &str) -> bool {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};

    /// 标记:该 env 存在即表示"已经伪装过,别再 exec 一次"。
    const MARKER: &str = "RSCAT_COMM_AS";

    if std::env::var_os(MARKER).is_some() {
        return true; // 已在伪装进程中
    }
    // 只接受纯文件名,杜绝路径穿越与怪异字符
    if name.is_empty()
        || name.len() > 32
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
    {
        return false;
    }

    let me = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return false,
    };

    // 目录名含 pid、可预测,多用户 /tmp 下可能被他人预置(symlink 攻击 /
    // TOCTOU)。先清掉同名旧路径,再以 0700 独占创建;任一失败就放弃伪装
    // (退回原进程名,功能降级但安全)。exec 后 PID 不变,新进程据同一 PID
    // 找回并清理它。
    let dir = std::env::temp_dir().join(format!("rscat-comm-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut builder = std::fs::DirBuilder::new();
    builder.mode(0o700);
    if builder.create(&dir).is_err() {
        return false;
    }
    let link = dir.join(name);

    // 硬链接优先(零拷贝);失败(跨卷/文件系统不支持)则退化为复制
    if std::fs::hard_link(&me, &link).is_err() {
        if std::fs::copy(&me, &link).is_err() {
            let _ = std::fs::remove_dir_all(&dir);
            return false;
        }
        if let Ok(md) = std::fs::metadata(&me) {
            let _ = std::fs::set_permissions(&link, std::fs::Permissions::from_mode(md.permissions().mode()));
        }
    }

    let link_c = match CString::new(link.as_os_str().as_encoded_bytes()) {
        Ok(c) => c,
        Err(_) => {
            let _ = std::fs::remove_dir_all(&dir);
            return false;
        }
    };

    // 原始 argv(跳过 argv[0])—— 用户可见的命令行原样保留
    let mut args: Vec<CString> = vec![link_c.clone()];
    for a in std::env::args().skip(1) {
        if let Ok(c) = CString::new(a) {
            args.push(c);
        }
    }
    let mut ptrs: Vec<*const libc::c_char> = args.iter().map(|c| c.as_ptr()).collect();
    ptrs.push(std::ptr::null());

    // 标记走环境变量:新进程的 env 自动继承,无需改动 argv 解析
    std::env::set_var(MARKER, name);

    unsafe {
        libc::execv(link_c.as_ptr(), ptrs.as_ptr());
    }
    // exec 失败:撤销标记与临时目录,让调用方按"未伪装"继续
    std::env::remove_var(MARKER);
    let _ = std::fs::remove_dir_all(&dir);
    false
}

#[cfg(not(target_os = "macos"))]
pub fn reexec_as(_name: &str) -> bool {
    true
}

/// macOS:删除 re-exec 留下的临时文件(p_comm 在 exec 时已定型,删文件不影响)。
/// PID 跨 exec 不变,故按自身 PID 即可找回。
#[cfg(target_os = "macos")]
pub fn cleanup_after_reexec() {
    if std::env::var_os("RSCAT_COMM_AS").is_none() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("rscat-comm-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(not(target_os = "macos"))]
pub fn cleanup_after_reexec() {}

/// 在伪终端里跑 cmd。
/// comm:代理期间 rscat 的进程名伪装(调用 shell 的名字)。fastfetch 等按
/// 父进程链报 SHELL/终端:shell 模块取直接父进程(包装 shell,本尊);
/// 终端模块跳过 shell 往上找第一个非 shell —— 若 rscat 保持自己的名字
/// 会被当成终端显示 "rscat";伪装成 shell 后 fastfetch 会穿过它命中
/// 链上真正的终端模拟器(kitty 等),连版本号都是真终端应答的。
/// env:需要注入子进程的环境变量(SHELL 指向真实调用 shell、zsh 的 ZDOTDIR 等)。
/// stop_file:会话标记文件;若给出且文件消失则结束(供 -a/-c 用)。
/// 返回子进程退出码。
pub fn run(
    cmd: &[String],
    env: &[(String, String)],
    comm: Option<&str>,
    flt: &mut LolcatFilter,
    stop_file: Option<&Path>,
    watch_parent: bool,
) -> i32 {
    if cmd.is_empty() {
        return 0;
    }
    unsafe {
        let mfd = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
        if mfd < 0 {
            return 127;
        }
        if libc::grantpt(mfd) != 0 || libc::unlockpt(mfd) != 0 {
            libc::close(mfd);
            return 127;
        }
        let name_ptr = libc::ptsname(mfd);
        if name_ptr.is_null() {
            libc::close(mfd);
            return 127;
        }
        let name = std::ffi::CStr::from_ptr(name_ptr).to_bytes().to_vec();
        let sfd = libc::open(
            CString::new(name).unwrap().as_ptr(),
            libc::O_RDWR | libc::O_NOCTTY,
        );
        if sfd < 0 {
            libc::close(mfd);
            return 127;
        }
        copy_winsize(sfd);
        // watch_parent 只在 -a 会话里为 true;交互式 shell 必须保留 ECHO,
        // 否则 readline 不回显用户输入(详见 slave_termios 文档)。
        slave_termios(sfd, keep_slave_echo(watch_parent));

        // argv
        let cstrs: Vec<CString> = cmd
            .iter()
            .map(|s| CString::new(s.as_str()).unwrap())
            .collect();
        let mut argv: Vec<*const libc::c_char> = cstrs.iter().map(|c| c.as_ptr()).collect();
        argv.push(std::ptr::null());

        let pid = libc::fork();
        if pid < 0 {
            libc::close(mfd);
            libc::close(sfd);
            return 127;
        }
        if pid == 0 {
            // ---- 子进程 ----
            libc::setsid();
            // 把从端设为控制终端。这里必须逐平台处理 ioctl 的 request 类型,
            // 因为 libc 各目标对它的声明并不一致:
            //
            //   macOS      TIOCSCTTY 是 c_uint,ioctl 的 request 是 c_ulong
            //              -> 必须转换,否则编译失败
            //   linux-gnu  TIOCSCTTY 是 Ioctl(=c_ulong),request 也是 c_ulong
            //              -> 转换是 no-op
            //   linux-musl TIOCSCTTY 是 Ioctl(=c_int),request 也是 c_int
            //              -> 转换到 c_ulong 反而编译失败(aarch64 包用的就是它)
            //   freebsd    TIOCSCTTY 是 c_ulong,request 也是 c_ulong
            //              -> 转换是 no-op
            //
            // 用 cfg 分开写,而不是统一 `as c_ulong`:后者在 musl 上会报
            // E0308 mismatched types,把 aarch64 静态包挡在编译阶段。
            #[cfg(target_os = "macos")]
            let sctty_request = libc::TIOCSCTTY as libc::c_ulong;
            #[cfg(not(target_os = "macos"))]
            let sctty_request = libc::TIOCSCTTY;
            let _ = libc::ioctl(sfd, sctty_request, 0);
            // 新 pty 的前台进程组未设置(TIOCGPGRP=0):shell 检测到
            // tcgetpgrp != pgrp 时会禁用行编辑并因 SIGTTIN 停住
            // (症状:嵌套 bash/zsh 输入不显示,回车才执行)。
            let _ = libc::tcsetpgrp(sfd, libc::getpgrp());
            for (k, v) in env {
                if let (Ok(k), Ok(v)) = (
                    CString::new(k.as_str()),
                    CString::new(v.as_str()),
                ) {
                    libc::setenv(k.as_ptr(), v.as_ptr(), 1);
                }
            }
            libc::dup2(sfd, libc::STDIN_FILENO);
            libc::dup2(sfd, libc::STDOUT_FILENO);
            libc::dup2(sfd, libc::STDERR_FILENO);
            if sfd > libc::STDERR_FILENO {
                libc::close(sfd);
            }
            libc::close(mfd);
            libc::execvp(argv[0], argv.as_ptr());
            let _ = std::io::stderr().write_all(b"rscat: exec failed\n");
            libc::_exit(127);
        }
        // ---- 父进程 ----
        libc::close(sfd);
        // 代理期间把 comm 伪装成调用 shell 的名字(见 run() 文档)
        if let Some(c) = comm {
            set_comm(c);
        }
        libc::signal(libc::SIGINT, on_sigint as *const () as libc::sighandler_t);
        libc::signal(libc::SIGWINCH, on_sigwinch as *const () as libc::sighandler_t);
        GOT_SIGINT.store(false, Ordering::Relaxed);
        GOT_SIGWINCH.store(false, Ordering::Relaxed);

        let saved = stdin_cbreak();
        let initial_ppid = libc::getppid(); // 父 shell(启动 rscat 的终端)
        // 永远尝试把父进程 stdin 转给子 pty。
        // - stdin 是终端:stdin_cbreak 已把它切成 cbreak(逐字节、无 ICANON),
        //   poll_stdin 原本就为 true。
        // - stdin 不是终端(管道 / 重定向 / 某些启动器):之前 poll_stdin 会是
        //   false,代理完全不读 stdin,于是会话收不到任何输入 —— 表现就是
        //   "按回车只换行、命令不执行"。改成始终轮询 stdin,并在非 tty 时
        //   置 O_NONBLOCK,使 select 循环能非阻塞地读取而不被卡死。
        let mut poll_stdin = true;
        let mut stdin_was_blocking = false;
        if saved.is_none() {
            stdin_was_blocking = set_nonblocking(libc::STDIN_FILENO);
        }
        let mut rc = 0;
        let mut child_gone = false;
        let mut stop_sent = false;
        let mut stop_at: Option<std::time::Instant> = None;
        let mut buf = [0u8; 65536];
        let mut kbd_pending: Vec<u8> = Vec::new();
        let mut kbd_idle: u8 = 0;

        loop {
            if GOT_SIGWINCH.swap(false, Ordering::Relaxed) {
                // 窗口变了:把新尺寸同步给子伪终端(从真 stdout 读)
                let mut ws: libc::winsize = std::mem::zeroed();
                if libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) == 0 {
                    // 通过主端设置从端尺寸:对主端用 TIOCSWINSZ 同样生效
                    libc::ioctl(mfd, libc::TIOCSWINSZ, &ws);
                }
            }
            if GOT_SIGINT.swap(false, Ordering::Relaxed) {
                // 转发给子进程(交互式 shell 取消当前行,非交互等价于终止)
                libc::kill(pid, libc::SIGINT);
            }
            if watch_parent && !stop_sent && libc::getppid() != initial_ppid {
                // 父 shell 已消失(终端被关闭等):这是无人认领的幽灵会话,
                // 收尾退出,避免占用标记文件让新会话误判"已在运行"。
                libc::kill(pid, libc::SIGHUP);
                stop_sent = true;
                stop_at = Some(std::time::Instant::now());
            }
            if let Some(p) = stop_file {
                if !stop_sent && !p.exists() {
                    // 会话被 rscat -c 取消:先 SIGHUP(交互式 shell 会正常退出,
                    // 它们通常忽略 SIGTERM),500ms 后还活着则 SIGKILL。
                    libc::kill(pid, libc::SIGHUP);
                    stop_sent = true;
                    stop_at = Some(std::time::Instant::now());
                }
                if stop_sent && !child_gone {
                    if let Some(t0) = stop_at {
                        if t0.elapsed() > std::time::Duration::from_millis(500) {
                            libc::kill(pid, libc::SIGKILL);
                        }
                    }
                }
            }

            let mut rfds: libc::fd_set = std::mem::zeroed();
            libc::FD_ZERO(&mut rfds);
            libc::FD_SET(mfd, &mut rfds);
            if poll_stdin {
                libc::FD_SET(libc::STDIN_FILENO, &mut rfds);
            }
            let maxfd = mfd.max(libc::STDIN_FILENO);
            let mut tv = libc::timeval {
                tv_sec: 0,
                tv_usec: 20000,
            };
            let n = libc::select(maxfd + 1, &mut rfds, std::ptr::null_mut(), std::ptr::null_mut(), &mut tv);
            if n < 0 {
                continue; // EINTR 等,重试
            }
            let mut mfd_hit = false;
            if libc::FD_ISSET(mfd, &rfds) {
                mfd_hit = true;
                let r = libc::read(mfd, buf.as_mut_ptr() as *mut libc::c_void, buf.len());
                if r <= 0 {
                    break; // EIO/EOF:子端已关
                }
                // 先代答 kitty 键盘协议查询(否则 fish 会启用 CSI-u 编码而
                // 回车失效,见 kbd_query_filter 文档)
                kbd_query_filter(&buf[..r as usize], &mut kbd_pending, mfd, flt);
                kbd_idle = 0;
            }
            // 扣住的「查询前缀」若长时间没有续上,说明那不是查询而是被切断的
            // 普通输出 —— 放行,免得子进程永远等不到应答或输出被吞。
            if kbd_pending.is_empty() {
                kbd_idle = 0;
            } else if !mfd_hit {
                kbd_idle = kbd_idle.saturating_add(1);
                if kbd_idle > 25 {
                    let stale = std::mem::take(&mut kbd_pending);
                    flt.feed(&stale);
                    kbd_idle = 0;
                }
            }
            if poll_stdin && libc::FD_ISSET(libc::STDIN_FILENO, &rfds) {
                // tty(cbreak):读一次就返回。非 tty(已置 O_NONBLOCK):循环读到
                // EAGAIN/EOF。绝不能对阻塞型 tty 循环读 —— 第二次 read 无数据时会
                // 永远阻塞,导致不再读 mfd/不再回收子进程(会话全面冻结)。
                loop {
                    let r = libc::read(
                        libc::STDIN_FILENO,
                        buf.as_mut_ptr() as *mut libc::c_void,
                        buf.len(),
                    );
                    if r > 0 {
                        write_all(mfd, &buf[..r as usize]);
                        if saved.is_some() {
                            break; // tty 路径:单次读
                        }
                    } else if r == 0 {
                        poll_stdin = false; // stdin EOF:不再轮询
                        break;
                    } else {
                        let err = std::io::Error::last_os_error();
                        // EAGAIN:非阻塞且暂无数据,留待下次 select;其他错误停止。
                        if err.raw_os_error() == Some(libc::EAGAIN) {
                            break;
                        }
                        poll_stdin = false;
                        break;
                    }
                }
            }
            // 子进程是否已退出
            let mut status = 0;
            let w = libc::waitpid(pid, &mut status, libc::WNOHANG);
            if w == pid {
                child_gone = true;
                if libc::WIFEXITED(status) {
                    rc = libc::WEXITSTATUS(status);
                } else if libc::WIFSIGNALED(status) {
                    rc = 128 + libc::WTERMSIG(status);
                }
                if !mfd_hit {
                    // 给残余输出 50ms 排空窗口
                    let mut rfds2: libc::fd_set = std::mem::zeroed();
                    libc::FD_ZERO(&mut rfds2);
                    libc::FD_SET(mfd, &mut rfds2);
                    let mut tv2 = libc::timeval {
                        tv_sec: 0,
                        tv_usec: 50000,
                    };
                    if libc::select(
                        mfd + 1,
                        &mut rfds2,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        &mut tv2,
                    ) <= 0
                    {
                        break;
                    }
                    // 还有数据:下轮循环读
                    continue;
                }
            }
            if child_gone {
                // 退出后多转几圈确保排空
                let mut idle = 0;
                while idle < 3 {
                    let mut rfds3: libc::fd_set = std::mem::zeroed();
                    libc::FD_ZERO(&mut rfds3);
                    libc::FD_SET(mfd, &mut rfds3);
                    let mut tv3 = libc::timeval {
                        tv_sec: 0,
                        tv_usec: 20000,
                    };
                    if libc::select(
                        mfd + 1,
                        &mut rfds3,
                        std::ptr::null_mut(),
                        std::ptr::null_mut(),
                        &mut tv3,
                    ) <= 0
                    {
                        idle += 1;
                        continue;
                    }
                    let r = libc::read(mfd, buf.as_mut_ptr() as *mut libc::c_void, buf.len());
                    if r <= 0 {
                        break;
                    }
                    flt.feed(&buf[..r as usize]);
                    idle = 0;
                }
                break;
            }
            // stop_file 消失且子进程还活着:等它收到 SIGTERM 退出(上面的 waitpid 会收)
            if stop_file.is_some_and(|p| !p.exists()) && !child_gone {
                // 再给一点时间让 SIGTERM 生效,避免忙等
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }

        if let Some(s) = saved {
            restore_stdin(&s);
        } else if stdin_was_blocking {
            // 非 tty 路径曾把 stdin 置为非阻塞,还原阻塞标志,避免泄漏给调用方。
            clear_nonblocking(libc::STDIN_FILENO);
        }
        if comm.is_some() {
            set_comm("rscat"); // 还原进程名
        }
        libc::signal(libc::SIGINT, libc::SIG_DFL);
        libc::signal(libc::SIGWINCH, libc::SIG_DFL);
        // 确保子进程已回收
        if !child_gone {
            let mut status = 0;
            libc::waitpid(pid, &mut status, 0);
            if libc::WIFEXITED(status) {
                rc = libc::WEXITSTATUS(status);
            } else if libc::WIFSIGNALED(status) {
                rc = 128 + libc::WTERMSIG(status);
            }
        }
        libc::close(mfd);
        rc
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// -a 会话(watch_parent=true)必须保留 ECHO;-e 运行模式必须关掉 ECHO
    /// (代理注入的终端查询应答不能被回显)。这是《用户输入不显示》bug 的策略层。
    #[test]
    fn session_keeps_echo_run_mode_disables_it() {
        assert!(keep_slave_echo(true), "-a session must keep ECHO on");
        assert!(!keep_slave_echo(false), "-e run mode must keep ECHO off");
    }

    /// 端到端验证 slave_termios 对真实 pty 的作用:开一个 pty 对,
    /// 分别按 -a / -e 策略设置从端,读回 ECHO 标志。
    #[test]
    fn slave_termios_sets_echo_flag_per_mode() {
        unsafe {
            let mfd = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
            assert!(mfd >= 0, "posix_openpt failed");
            assert_eq!(libc::grantpt(mfd), 0);
            assert_eq!(libc::unlockpt(mfd), 0);
            let name = libc::ptsname(mfd);
            assert!(!name.is_null());
            let sfd = libc::open(name, libc::O_RDWR | libc::O_NOCTTY);
            assert!(sfd >= 0, "open slave failed");

            // 先人为关掉 ECHO,模拟内核默认/上次遗留状态。
            let mut t: libc::termios = std::mem::zeroed();
            assert_eq!(libc::tcgetattr(sfd, &mut t), 0);
            t.c_lflag &= !libc::ECHO;
            assert_eq!(libc::tcsetattr(sfd, libc::TCSANOW, &t), 0);

            // -a 会话:应恢复 ECHO。
            slave_termios(sfd, keep_slave_echo(true));
            let mut after: libc::termios = std::mem::zeroed();
            assert_eq!(libc::tcgetattr(sfd, &mut after), 0);
            assert!(
                after.c_lflag & libc::ECHO != 0,
                "session mode must leave ECHO on (readline needs it to echo input)"
            );

            // -e 运行模式:应关掉 ECHO。
            slave_termios(sfd, keep_slave_echo(false));
            let mut after2: libc::termios = std::mem::zeroed();
            assert_eq!(libc::tcgetattr(sfd, &mut after2), 0);
            assert!(
                after2.c_lflag & libc::ECHO == 0,
                "run mode must keep ECHO off (proxy answers must not echo)"
            );

            // 非 canonical(ICANON)必须保留:cooked 模式是排版正确的前提。
            assert!(
                after2.c_lflag & libc::ICANON != 0,
                "slave must stay cooked (ICANON on)"
            );

            libc::close(sfd);
            libc::close(mfd);
        }
    }
    /// 代理必须清掉父端 stdin 的 ICRNL。
    ///
    /// 回归场景(真 fish 终端里「回车只换行、命令不执行」):终端驱动默认
    /// ICRNL 会把用户按下的回车 CR(0x0d) 改写成 LF(0x0a) 才交给我们转发。
    /// bash/zsh 的 readline 认为 LF 也能提交,所以看不出问题;fish 自己关掉
    /// ICANON 做行编辑,只认 CR=执行、LF=插入换行 —— 于是命令永远不执行。
    /// 修法是 cbreak 时同时清 ICRNL,让 CR 原样过桥。
    #[test]
    fn cbreak_clears_icrnl_so_enter_survives() {
        unsafe {
            let mfd = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
            assert!(mfd >= 0, "posix_openpt failed");
            assert_eq!(libc::grantpt(mfd), 0);
            assert_eq!(libc::unlockpt(mfd), 0);
            let name = libc::ptsname(mfd);
            assert!(!name.is_null());
            let sfd = libc::open(name, libc::O_RDWR | libc::O_NOCTTY);
            assert!(sfd >= 0, "open slave failed");

            let mut t: libc::termios = std::mem::zeroed();
            assert_eq!(libc::tcgetattr(sfd, &mut t), 0);
            t.c_iflag |= libc::ICRNL;
            assert_eq!(libc::tcsetattr(sfd, libc::TCSANOW, &t), 0);
            assert!(t.c_iflag & libc::ICRNL != 0, "precondition: ICRNL set");

            // cbreak 的变换规则:清 ICANON|ECHO|c_iflag 的 ICRNL。
            let mut want = t;
            want.c_lflag &= !(libc::ICANON | libc::ECHO);
            want.c_iflag &= !libc::ICRNL;
            assert_eq!(
                want.c_iflag & libc::ICRNL,
                0,
                "cbreak must clear ICRNL: otherwise CR becomes LF and fish never executes"
            );
            assert_eq!(
                want.c_lflag & (libc::ICANON | libc::ECHO),
                0,
                "cbreak must clear ICANON|ECHO"
            );

            libc::close(sfd);
            libc::close(mfd);
        }
    }

    /// 查询被 read() 从中间切断时,前缀要留在 pending 里等续包。
    #[test]
    fn kitty_query_split_across_chunks_is_held() {
        unsafe {
            let mfd = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
            assert!(mfd >= 0);
            libc::grantpt(mfd);
            libc::unlockpt(mfd);
            let nm = libc::ptsname(mfd);
            let sfd = libc::open(nm, libc::O_RDWR | libc::O_NOCTTY);
            assert!(sfd >= 0);

            let sink = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
            struct S(std::rc::Rc<std::cell::RefCell<Vec<u8>>>);
            impl std::io::Write for S {
                fn write(&mut self, d: &[u8]) -> std::io::Result<usize> {
                    self.0.borrow_mut().extend_from_slice(d);
                    Ok(d.len())
                }
                fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
            }
            let mut flt = crate::filter::LolcatFilter::new(
                Box::new(S(sink.clone())),
                0.1, 3.0, 1,
                crate::rainbow::Painter::new(crate::rainbow::ColorMode::C256, false),
                false, 1, 20.0,
            );
            // 从端设为 raw + 非阻塞:应答是给子进程读的,测试里直接读从端,
            // canonical 模式下 read 会一直等换行而挂住(实测踩过)。
            let mut rt: libc::termios = std::mem::zeroed();
            assert_eq!(libc::tcgetattr(sfd, &mut rt), 0);
            rt.c_lflag &= !(libc::ICANON | libc::ECHO);
            rt.c_cc[libc::VMIN] = 0;
            rt.c_cc[libc::VTIME] = 0;
            assert_eq!(libc::tcsetattr(sfd, libc::TCSANOW, &rt), 0);
            let fl = libc::fcntl(sfd, libc::F_GETFL, 0);
            libc::fcntl(sfd, libc::F_SETFL, fl | libc::O_NONBLOCK);

            let mut pending: Vec<u8> = Vec::new();

            // 第一批只到 "\x1b[" —— 可能是查询前缀,应扣住不上行。
            kbd_query_filter(b"\x1b[", &mut pending, mfd, &mut flt);
            assert_eq!(pending, b"\x1b[", "partial query prefix must be held");
            assert!(sink.borrow().is_empty(), "held prefix must not be emitted yet");

            // 第二批补齐 -> 代答,且不残留。
            kbd_query_filter(b"?u", &mut pending, mfd, &mut flt);
            assert!(pending.is_empty(), "pending must drain once query completes");
            let mut resp = [0u8; 8];
            let n = libc::read(sfd, resp.as_mut_ptr() as *mut libc::c_void, resp.len());
            assert!(n > 0, "completed query must be answered");
            assert_eq!(&resp[..n as usize], KBD_ANSWER);

            libc::close(sfd);
            libc::close(mfd);
        }
    }
}
