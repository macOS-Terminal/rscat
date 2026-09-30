//! i18n: 简体中文 / 繁體中文 / English / 日本語。
//! 语言检测优先级: `--lang` > `RSCAT_LANG` > `LC_ALL` > `LC_MESSAGES` > `LANG` > en。

/// 用户可见版本号(不含 `rscat ` 前缀):Cargo 基础版本 + 打包修订号。
/// 全项目唯一来源:`--version` 与 help 首行都取这里;打包脚本(build/,本地)
/// 也按这个修订号命名产物。升级时改 `Cargo.toml` 的 version,或改这里的 -N。
pub const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "-5");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    ZhCn,
    ZhTw,
    En,
    Ja,
}

impl Lang {
    /// 解析语言标签,大小写/下划线/编码后缀均归一化。未知返回 None。
    pub fn parse(s: &str) -> Option<Lang> {
        let mut t = s.trim().to_lowercase().replace('_', "-");
        if let Some(i) = t.find('.') {
            t.truncate(i);
        }
        if let Some(i) = t.find('@') {
            t.truncate(i);
        }
        match t.as_str() {
            "zh-cn" | "zh-hans" | "zh-sg" | "zh" | "chinese" => Some(Lang::ZhCn),
            "zh-tw" | "zh-hk" | "zh-mo" | "zh-hant" => Some(Lang::ZhTw),
            "ja" | "ja-jp" | "japanese" => Some(Lang::Ja),
            "en" | "en-us" | "en-gb" | "english" | "c" | "posix" => Some(Lang::En),
            _ => {
                if t.starts_with("zh-tw") || t.starts_with("zh-hk") || t.starts_with("zh-hant") {
                    Some(Lang::ZhTw)
                } else if t.starts_with("zh") {
                    Some(Lang::ZhCn)
                } else if t.starts_with("ja") {
                    Some(Lang::Ja)
                } else if t.starts_with("en") {
                    Some(Lang::En)
                } else {
                    None
                }
            }
        }
    }

    /// 自动检测,永不失败(兜底英文)。
    pub fn detect() -> Lang {
        for key in ["RSCAT_LANG", "LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(v) = std::env::var(key) {
                if !v.trim().is_empty() {
                    if let Some(l) = Lang::parse(&v) {
                        return l;
                    }
                }
            }
        }
        Lang::En
    }
}

/// 所有用户可见字符串。新增文案时请四个语言一起加。
#[derive(Debug, Clone, Copy)]
pub enum Msg {
    // ---- 错误 ----
    ErrNeedUnixPty,
    ErrReadFile,
    ErrNoDecode,
    ErrConflictAlwaysExec,
    ErrUnknownShell,
    ErrUnknownLang,
    ErrBadNumber,
    ErrMissingValue,
    ErrUnknownFlag,
    ErrExecFailed,
    // ---- 会话 ----
    SessionEnter,
    SessionExit,
    SessionAlready,
    SessionNested,
    CancelOk,
    CancelNone,
    ShellFallback,
}

pub fn t(lang: Lang, m: Msg) -> &'static str {
    use Lang::*;
    use Msg::*;
    match (lang, m) {
        // ---- 错误 ----
        (ZhCn, ErrNeedUnixPty) => "rscat: 运行模式需要 Unix pty 支持,当前平台暂不支持",
        (ZhTw, ErrNeedUnixPty) => "rscat: 執行模式需要 Unix pty 支援,目前平台暫不支援",
        (En, ErrNeedUnixPty) => "rscat: run mode needs Unix pty support, not available on this platform",
        (Ja, ErrNeedUnixPty) => "rscat: 実行モードには Unix pty が必要です(この環境では未対応)",
        (ZhCn, ErrReadFile) => "rscat: 读不了",
        (ZhTw, ErrReadFile) => "rscat: 讀不了",
        (En, ErrReadFile) => "rscat: cannot read",
        (Ja, ErrReadFile) => "rscat: 読めません",
        (ZhCn, ErrNoDecode) => "rscat: 无法解码(需要 PNG/JPEG/GIF/BMP/WebP,或改用 --proto iterm)",
        (ZhTw, ErrNoDecode) => "rscat: 無法解碼(需要 PNG/JPEG/GIF/BMP/WebP,或改用 --proto iterm)",
        (En, ErrNoDecode) => "rscat: cannot decode (need PNG/JPEG/GIF/BMP/WebP, or use --proto iterm)",
        (Ja, ErrNoDecode) => "rscat: デコードできません(PNG/JPEG/GIF/BMP/WebP が必要、または --proto iterm を使用)",
        (ZhCn, ErrConflictAlwaysExec) => "rscat: -a(会话)与 -e(执行)不能同时用",
        (ZhTw, ErrConflictAlwaysExec) => "rscat: -a(工作階段)與 -e(執行)不能同時用",
        (En, ErrConflictAlwaysExec) => "rscat: -a (session) and -e (exec) cannot be combined",
        (Ja, ErrConflictAlwaysExec) => "rscat: -a(セッション)と -e(実行)は併用できません",
        (ZhCn, ErrUnknownShell) => "rscat: 未知的 shell 类型,用 --init {bash,zsh,sh,fish,powershell,cmd}",
        (ZhTw, ErrUnknownShell) => "rscat: 未知的 shell 類型,用 --init {bash,zsh,sh,fish,powershell,cmd}",
        (En, ErrUnknownShell) => "rscat: unknown shell, use --init {bash,zsh,sh,fish,powershell,cmd}",
        (Ja, ErrUnknownShell) => "rscat: 不明なシェルです --init {bash,zsh,sh,fish,powershell,cmd} を使用",
        (ZhCn, ErrUnknownLang) => "rscat: 未知的语言,用 --lang {zh-CN,zh-TW,en,ja}",
        (ZhTw, ErrUnknownLang) => "rscat: 未知的語言,用 --lang {zh-CN,zh-TW,en,ja}",
        (En, ErrUnknownLang) => "rscat: unknown language, use --lang {zh-CN,zh-TW,en,ja}",
        (Ja, ErrUnknownLang) => "rscat: 不明な言語です --lang {zh-CN,zh-TW,en,ja} を使用",
        (ZhCn, ErrBadNumber) => "rscat: 数值不合法",
        (ZhTw, ErrBadNumber) => "rscat: 數值不合法",
        (En, ErrBadNumber) => "rscat: invalid number",
        (Ja, ErrBadNumber) => "rscat: 数値が不正です",
        (ZhCn, ErrMissingValue) => "rscat: 选项缺参数",
        (ZhTw, ErrMissingValue) => "rscat: 選項缺參數",
        (En, ErrMissingValue) => "rscat: option needs a value",
        (Ja, ErrMissingValue) => "rscat: オプションに値が必要です",
        (ZhCn, ErrUnknownFlag) => "rscat: 未知选项",
        (ZhTw, ErrUnknownFlag) => "rscat: 未知選項",
        (En, ErrUnknownFlag) => "rscat: unknown option",
        (Ja, ErrUnknownFlag) => "rscat: 不明なオプションです",
        (ZhCn, ErrExecFailed) => "rscat: 启动失败",
        (ZhTw, ErrExecFailed) => "rscat: 啟動失敗",
        (En, ErrExecFailed) => "rscat: failed to start",
        (Ja, ErrExecFailed) => "rscat: 起動に失敗しました",
        // ---- 会话 ----
        (ZhCn, SessionEnter) => "🌈 已进入彩虹会话:之后所有命令输出都是彩虹色,输入 exit 或运行 rscat -c 退出",
        (ZhTw, SessionEnter) => "🌈 已進入彩虹工作階段:之後所有命令輸出都是彩虹色,輸入 exit 或執行 rscat -c 離開",
        (En, SessionEnter) => "🌈 Rainbow session started: all command output from now on is rainbow. Type exit or run rscat -c to leave",
        (Ja, SessionEnter) => "🌈 虹セッション開始:これ以降すべての出力が虹色になります。exit または rscat -c で終了",
        (ZhCn, SessionExit) => "🌈 已退出彩虹会话",
        (ZhTw, SessionExit) => "🌈 已離開彩虹工作階段",
        (En, SessionExit) => "🌈 Rainbow session ended",
        (Ja, SessionExit) => "🌈 虹セッションを終了しました",
        (ZhCn, SessionAlready) => "rscat: 彩虹会话已在运行",
        (ZhTw, SessionAlready) => "rscat: 彩虹工作階段已在執行",
        (En, SessionAlready) => "rscat: rainbow session already running",
        (Ja, SessionAlready) => "rscat: 虹セッションは既に実行中です",
        (ZhCn, SessionNested) => "rscat: 已在彩虹会话里了,不要嵌套",
        (ZhTw, SessionNested) => "rscat: 已在彩虹工作階段裡了,不要巢狀",
        (En, SessionNested) => "rscat: already inside a rainbow session, refusing to nest",
        (Ja, SessionNested) => "rscat: 既に虹セッション内です(入れ子は不可)",
        (ZhCn, CancelOk) => "rscat: 已取消彩虹会话",
        (ZhTw, CancelOk) => "rscat: 已取消彩虹工作階段",
        (En, CancelOk) => "rscat: rainbow session cancelled",
        (Ja, CancelOk) => "rscat: 虹セッションを解除しました",
        (ZhCn, CancelNone) => "rscat: 当前没有彩虹会话",
        (ZhTw, CancelNone) => "rscat: 目前沒有彩虹工作階段",
        (En, CancelNone) => "rscat: no rainbow session running",
        (Ja, CancelNone) => "rscat: 虹セッションは実行されていません",
        (ZhCn, ShellFallback) => "rscat: 找不到 SHELL,用 /bin/sh",
        (ZhTw, ShellFallback) => "rscat: 找不到 SHELL,用 /bin/sh",
        (En, ShellFallback) => "rscat: SHELL not found, using /bin/sh",
        (Ja, ShellFallback) => "rscat: SHELL が見つからないため /bin/sh を使用",
    }
}

/// 多语言帮助全文(-h)。与 lolcat 选项兼容,另加 rscat 特有项。
/// 文本里的 `{VERSION}` 占位由 `VERSION` 常量填充,避免在四个文件里各写一份版本号。
pub fn help(lang: Lang) -> String {
    let raw = match lang {
        Lang::ZhCn => include_str!("help_zh_cn.txt"),
        Lang::ZhTw => include_str!("help_zh_tw.txt"),
        Lang::En => include_str!("help_en.txt"),
        Lang::Ja => include_str!("help_ja.txt"),
    };
    raw.replace("{VERSION}", VERSION)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 版本号单一来源:Cargo 基础版本 + 打包修订号。
    #[test]
    fn version_is_single_sourced() {
        assert_eq!(VERSION, concat!(env!("CARGO_PKG_VERSION"), "-5"));
    }

    /// help 文本里的 {VERSION} 占位必须被填掉,四种语言都不得残留。
    #[test]
    fn help_substitutes_version_placeholder() {
        for lang in [Lang::ZhCn, Lang::ZhTw, Lang::En, Lang::Ja] {
            let h = help(lang);
            assert!(
                h.starts_with("rscat "),
                "help must start with 'rscat ': {:?}",
                &h[..h.len().min(20)]
            );
            assert!(h.contains(VERSION), "help must contain the version");
            assert!(!h.contains("{VERSION}"), "placeholder must be substituted");
        }
    }
}
