//! 会话持久化:彩虹会话标记文件。
//! 路径:Unix 上 $XDG_CONFIG_HOME/rscat/(默认 ~/.config/rscat/),
//! Windows 上 %APPDATA%/rscat/。
//! 每个会话一个独立标记文件 session-<rscat的pid>,多个会话互不干扰
//! (一个会话的清理不会误杀另一个);文件存在 = 会话开着,
//! `rscat -a` 创建,`rscat -c` 删除全部,会话主循环每轮检查
//! 自己的标记,消失即收尾退出。

use std::path::PathBuf;

pub fn config_dir() -> PathBuf {
    #[cfg(windows)]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            if !appdata.is_empty() {
                return PathBuf::from(appdata).join("rscat");
            }
        }
        // 兜底:用户目录
        if let Ok(home) = std::env::var("USERPROFILE") {
            return PathBuf::from(home).join("AppData").join("Roaming").join("rscat");
        }
        PathBuf::from("rscat")
    }
    #[cfg(not(windows))]
    {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            if !xdg.is_empty() {
                return PathBuf::from(xdg).join("rscat");
            }
        }
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(".config").join("rscat");
        }
        PathBuf::from(".config/rscat")
    }
}

/// 本会话的标记文件(session-<pid>)。
pub fn session_marker() -> PathBuf {
    config_dir().join(format!("session-{}", std::process::id()))
}

/// 是否存在任何彩虹会话(任意 session-* 标记)。
pub fn session_active() -> bool {
    session_markers().next().is_some()
}

/// 列出所有会话标记文件。
pub fn session_markers() -> impl Iterator<Item = PathBuf> {
    let dir = config_dir();
    (|| {
        let rd = std::fs::read_dir(dir).ok()?;
        Some(rd.flatten().map(|e| e.path()).filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with("session-"))
                .unwrap_or(false)
        }))
    })()
    .into_iter()
    .flatten()
}

/// 创建本会话的标记(父目录一并建好)。内容为 "rscat的pid 父shell的pid",
/// 供 cleanup_stale 判断会话是否已是无人认领的幽灵。失败返回 Err。
pub fn session_start() -> std::io::Result<()> {
    let m = session_marker();
    if let Some(dir) = m.parent() {
        std::fs::create_dir_all(dir)?;
    }
    #[cfg(unix)]
    let ppid = unsafe { libc::getppid() };
    // Windows:无 -a 会话(需 ConPTY),此函数实际不会被调到,仅保证可编译
    #[cfg(windows)]
    let ppid = 0u32;
    std::fs::write(&m, format!("{} {}", std::process::id(), ppid))?;
    Ok(())
}

/// 标记属主(rscat 进程)是否还活着。
/// 注意:不能读 comm —— `-a` 代理期间 comm 被伪装成调用 shell 的名字;
/// 用 /proc/<pid>/exe 的文件名判断(Linux,防 PID 复用误判),
/// 其他 Unix 用 kill(pid, 0) 探活。
fn owner_alive(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        match std::fs::read_link(format!("/proc/{pid}/exe")) {
            Ok(p) => p.file_name().map(|n| n == "rscat").unwrap_or(false),
            Err(_) => false,
        }
    }
    #[cfg(all(unix, not(target_os = "linux")))]
    {
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }
    // Windows:没有 -a 会话(需 ConPTY),标记一律视为无主
    #[cfg(windows)]
    {
        let _ = pid;
        false
    }
}

/// 清理陈旧标记:终端被关闭时 rscat 来不及收尾,标记会残留。
/// 陈旧判定:属主 rscat 已死,或属主还活着但其父 shell(启动 rscat 的
/// 交互终端)已经不在 —— 后者是无人认领的幽灵会话,给它发 SIGHUP
/// 让它收尾退出,并删除标记。
pub fn cleanup_stale() {
    for m in session_markers() {
        let Ok(content) = std::fs::read_to_string(&m) else {
            let _ = std::fs::remove_file(&m);
            continue;
        };
        let mut nums = content.split_whitespace().filter_map(|t| t.parse::<u32>().ok());
        let Some(owner) = nums.next() else {
            let _ = std::fs::remove_file(&m); // 旧格式/空内容:按陈旧处理
            continue;
        };
        if !owner_alive(owner) {
            let _ = std::fs::remove_file(m);
            continue;
        }
        if let Some(parent) = nums.next() {
            #[cfg(unix)]
            {
                // 幽灵会话:属主活着,但它的父 shell 已经没了
                let parent_gone = unsafe { libc::kill(parent as i32, 0) != 0 };
                if parent_gone {
                    unsafe { libc::kill(owner as i32, libc::SIGHUP) };
                    let _ = std::fs::remove_file(m);
                }
            }
            // Windows:无 -a 会话(需 ConPTY),父进程无从谈起,清掉残留标记即可
            #[cfg(windows)]
            {
                let _ = (parent, owner);
                let _ = std::fs::remove_file(&m);
            }
        }
    }
}

/// 删除本会话自己的标记(`session-<本进程 pid>`)。会话正常收尾时用。
/// 与 `session_stop` 不同:只删自己,绝不误删其他会话的标记 —— 否则一个
/// 会话退出会把并发的另一个会话也顶掉(它的主循环发现标记消失就会收尾)。
pub fn session_clear_own() {
    let _ = std::fs::remove_file(session_marker());
}

/// 删除全部会话标记(`rscat -c`:取消所有会话)。
pub fn session_stop() {
    for m in session_markers() {
        let _ = std::fs::remove_file(m);
    }
}

/// 是否正运行在 rscat 彩虹会话的子 shell 里(防嵌套)。
pub fn inside_session() -> bool {
    std::env::var("RSCAT_SESSION").as_deref() == Ok("1")
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// 真实文件系统验证标记删除语义(XDG_CONFIG_HOME 指向临时目录):
    ///   - `session_clear_own` 只删自己的标记(并发会话不被误杀);
    ///   - `session_stop`(`-c`)删全部。
    #[test]
    fn clear_own_keeps_others_stop_removes_all() {
        let old = std::env::var_os("XDG_CONFIG_HOME");
        let dir = std::env::temp_dir().join(format!("rscat-persist-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::env::set_var("XDG_CONFIG_HOME", &dir);

        session_start().expect("session_start");
        let own = session_marker();
        assert!(own.exists(), "own marker must exist after session_start");

        // 模拟另一个并发会话的标记
        let other = config_dir().join("session-999999");
        std::fs::write(&other, "999999 1").unwrap();

        session_clear_own();
        assert!(!own.exists(), "clear_own must remove the own marker");
        assert!(other.exists(), "clear_own must not touch another session");

        session_stop();
        assert!(!other.exists(), "session_stop must remove every marker");

        match old {
            Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
            None => std::env::remove_var("XDG_CONFIG_HOME"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
