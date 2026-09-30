//! 调用 shell 检测:从父进程链找出真正的 shell(fish/zsh/bash/…)。
//!
//! 背景:fastfetch 等工具按"父进程"报告 SHELL,rscat 直接 spawn 会让它们
//! 显示 "rscat"(用户实际在 fish/zsh 里)。检测出真实调用 shell 后:
//!   - `-a` 会话用同款交互 shell;
//!   - `-e` 用它包一层 `-c`(bash/zsh 须以 `; exit $?` 破掉"末尾单命令
//!     exec"优化,否则父进程穿帮;fish 无此优化且没有 `$?`);
//!   - 子进程 SHELL 环境变量指向检测到的 shell,所见即所用。

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// sh/dash/ash/ksh/mksh/bash/zsh:`-c 'cmd; exit $?'`
    Posix,
    /// fish:`-c 'cmd'`(不会 exec 优化,`$?` 不存在)
    Fish,
    /// PowerShell:`-Command`
    Pwsh,
    /// cmd.exe:`/C`
    Cmd,
}

#[derive(Debug, Clone)]
pub struct Shell {
    pub path: String,
    pub kind: Kind,
}

impl Shell {
    pub fn name(&self) -> String {
        Path::new(&self.path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.clone())
    }
}

fn classify(basename: &str) -> Option<Kind> {
    match basename.to_lowercase().as_str() {
        "fish" => Some(Kind::Fish),
        "pwsh" | "powershell" => Some(Kind::Pwsh),
        "cmd" | "cmd.exe" => Some(Kind::Cmd),
        "bash" | "zsh" | "sh" | "dash" | "ash" | "ksh" | "mksh" | "posh" => Some(Kind::Posix),
        _ => None,
    }
}

fn resolve_in_path(name: &str) -> Option<PathBuf> {
    if name.contains('/') {
        let p = PathBuf::from(name);
        return if p.exists() { Some(p) } else { None };
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|p| p.is_file())
}

/// 父进程链的 comm 名,自近及远,最多 max 跳。
#[cfg(target_os = "linux")]
fn parent_chain_comms(max: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut pid = std::process::id();
    for _ in 0..max {
        // comm 可能含括号,ppid 要从最后一个 ')' 之后取。
        // stat 格式:pid (comm) state ppid …→ 右半第一个是 state,第二个才是 ppid。
        let stat = match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(d) => d,
            Err(_) => break,
        };
        let ppid: u32 = match stat.rsplit_once(')').and_then(|(_, r)| {
            r.split_whitespace().nth(1).and_then(|s| s.parse().ok())
        }) {
            Some(p) => p,
            None => break,
        };
        match std::fs::read_to_string(format!("/proc/{pid}/comm")) {
            Ok(c) => out.push(c.trim().to_string()),
            Err(_) => break,
        }
        if ppid <= 1 {
            break;
        }
        pid = ppid;
    }
    out
}

#[cfg(all(unix, not(target_os = "linux")))]
fn parent_chain_comms(max: usize) -> Vec<String> {
    // macOS/FreeBSD 没有 Linux 式 /proc:用 ps 逐跳问
    let mut out = Vec::new();
    let mut pid = std::process::id();
    for _ in 0..max {
        let ask = |args: &[&str]| -> Option<String> {
            std::process::Command::new("ps")
                .args(args)
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        };
        let comm = match ask(&["-o", "comm=", "-p", &pid.to_string()]) {
            Some(c) if !c.is_empty() => c,
            _ => break,
        };
        out.push(comm);
        let ppid = ask(&["-o", "ppid=", "-p", &pid.to_string()]).unwrap_or_default();
        let ppid: u32 = ppid.parse().unwrap_or(1);
        if ppid <= 1 {
            break;
        }
        pid = ppid;
    }
    out
}

#[cfg(windows)]
fn parent_chain_comms(_max: usize) -> Vec<String> {
    // Windows 侧不做进程链检测,走 $SHELL/COMSPEC 兜底
    Vec::new()
}

/// 从父进程链检测调用 shell;找不到(父进程不是已知 shell)返回 None。
pub fn detect() -> Option<Shell> {
    for comm in parent_chain_comms(8) {
        let base = Path::new(&comm)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| comm.clone());
        if let Some(kind) = classify(&base) {
            if let Some(path) = resolve_in_path(&base) {
                return Some(Shell {
                    path: path.to_string_lossy().into_owned(),
                    kind,
                });
            }
        }
    }
    None
}

/// 真实终端名:从 rscat 的上一级开始沿父链跳过 shell,
/// 第一个非 shell 祖先就是终端模拟器(kitty/alacritty/…)。
/// 这正是 fastfetch 终端模块自己的判定逻辑(它按父进程找终端,
/// 跳过已知 shell),rscat 卡在中间会被当成终端显示 "rscat"。
/// (当前未使用:comm 伪装成调用 shell 名让 fastfetch 穿过本进程;
///  保留给未来需要显式终端名的场景。)
#[allow(dead_code)]
pub fn detect_terminal_name() -> Option<String> {
    for comm in parent_chain_comms(8).into_iter().skip(1) {
        // skip(1):跳过 rscat 自己
        let base = Path::new(&comm)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| comm.clone());
        if base == "rscat" {
            continue;
        }
        if classify(&base).is_some() {
            continue; // shell 祖先,继续往上
        }
        return Some(base);
    }
    None
}

/// 兜底:$SHELL(可识别时)→ Windows COMSPEC → /bin/sh。
pub fn fallback() -> Shell {
    if let Ok(s) = std::env::var("SHELL") {
        if !s.is_empty() {
            if let Some(kind) = Path::new(&s).file_name().and_then(|b| {
                classify(&b.to_string_lossy())
            }) {
                return Shell { path: s, kind };
            }
        }
    }
    #[cfg(windows)]
    {
        let comspec = std::env::var("COMSPEC")
            .ok()
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| "cmd.exe".to_string());
        Shell {
            path: comspec,
            kind: Kind::Cmd,
        }
    }
    #[cfg(not(windows))]
    {
        Shell {
            path: "/bin/sh".to_string(),
            kind: Kind::Posix,
        }
    }
}

/// sh/bash/zsh/fish 通用的单引号转义。
fn quote_sh(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// -e 运行模式:把待跑命令用调用 shell 包一层。
pub fn exec_argv(shell: &Shell, cmd: &[String]) -> Vec<String> {
    let mut joined = String::new();
    match shell.kind {
        Kind::Fish => {
            for (i, a) in cmd.iter().enumerate() {
                if i > 0 {
                    joined.push(' ');
                }
                joined.push_str(&quote_sh(a));
            }
            vec![shell.path.clone(), "-c".into(), joined]
        }
        Kind::Posix => {
            for (i, a) in cmd.iter().enumerate() {
                if i > 0 {
                    joined.push(' ');
                }
                joined.push_str(&quote_sh(a));
            }
            // `; exit $?` 保住退出码,同时破掉 bash/zsh 的末尾单命令 exec 优化
            joined.push_str("; exit $?");
            vec![shell.path.clone(), "-c".into(), joined]
        }
        Kind::Pwsh => {
            joined.push_str(&cmd.join(" "));
            vec![shell.path.clone(), "-Command".into(), joined]
        }
        Kind::Cmd => {
            let mut v = vec![shell.path.clone(), "/C".into()];
            v.extend(cmd.iter().cloned());
            v
        }
    }
}

/// -a 会话:交互式 shell 的 argv,以及需要注入子进程的环境变量。
/// zsh 首次交互若没有 ~/.zshrc 会跑 newuser 向导(且向导选项 2 会
/// cp 一个 Arch 上不存在的推荐文件而报错),用 ZDOTDIR 指向带最小
/// .zshrc 的临时目录压掉;用户已有 .zshrc 则不干预。
pub fn session_argv(shell: &Shell) -> (Vec<String>, Vec<(String, String)>) {
    let _ = shell;
    let mut env: Vec<(String, String)> = vec![
        // 会话内所有子 shell(含嵌套的 bash/zsh)统一发 OSC 133 标记:
        // bash 用 PROMPT_COMMAND/PS0,zsh 用 ZDOTDIR 里的钩子,
        // fish 用 config.fish 里的 RSCAT_SESSION 门控事件钩子。
        ("SHELL".into(), shell.path.clone()),
        ("PROMPT_COMMAND".into(), r#"printf "\033]133;A\007""#.into()),
        ("PS0".into(), r#"\033]133;C\007"#.into()),
    ];
    // ZDOTDIR:压 zsh newuser 向导 + precmd/preexec 发标记;
    // 用户的 ~/.zshrc 若存在则先 source,保留其配置。
    let dir = std::env::temp_dir().join(format!("rscat-zdot-{}", std::process::id()));
    if std::fs::create_dir_all(&dir).is_ok() {
        let rc = concat!(
            "# by rscat: suppress zsh-newuser-install + OSC 133 marks\n",
            "[[ -f $HOME/.zshrc ]] && source $HOME/.zshrc\n",
            "precmd()  { printf \"\\033]133;D\\007\\033]133;A\\007\"; }\n",
            "preexec() { printf \"\\033]133;C\\007\"; }\n"
        );
        if std::fs::write(dir.join(".zshrc"), rc).is_ok() {
            env.push(("ZDOTDIR".into(), dir.to_string_lossy().into_owned()));
        }
    }
    // fish 会话:用 -C 注入同样的事件钩子(fish 原生发 OSC 133 的前提是
    // 终端完成查询握手;显式标记保证提示符/输出边界一定被跟踪)。
    let argv = if shell.kind == Kind::Fish {
        vec![
            shell.path.clone(),
            "-C".into(),
            "function __rscat_preexec --on-event fish_preexec; printf \"\\033]133;C\\007\"; end;\
             function __rscat_precmd --on-event fish_prompt; printf \"\\033]133;D\\007\\033]133;A\\007\"; end".into(),
        ]
    } else {
        vec![shell.path.clone()]
    };
    (argv, env)
}

/// -e 模式注入子进程的环境(至少把 SHELL 指向真实调用 shell)。
pub fn exec_env(shell: &Shell) -> Vec<(String, String)> {
    vec![("SHELL".into(), shell.path.clone())]
}
