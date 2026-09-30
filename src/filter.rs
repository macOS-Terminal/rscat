//! 转义序列感知过滤器:与 nyacat Python 版 LolcatFilter 同模型。
//! 配对模型:每行切成 (转义连跑, 单个可见字符) 配对,配对序号 i 决定颜色
//! rainbow(freq, os + i/spread);os 每行 +1(首行 seed+1)。
//! 输出顺序:转义连跑原样 → 颜色 SGR → 字符 → 复位(含空字符配对)。
//! kitty APC / sixel DCS / OSC / CSI 原样直通,图片不被破坏。

use std::io::Write;

use crate::rainbow::{Painter, rainbow};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Ground,
    Esc,
    Csi,
    Osc,
    OscEsc,
    Str,
    /// ESC + 中间字节(0x20..=0x2F)之后的收尾字符。
    /// 覆盖 SCS 字符集指派 `ESC ( B` / `ESC ) B`、`ESC # 8` 等 3 字节转义。
    /// 少了这个状态,`ESC ( B` 会被当成"两字符转义"当场结束,随后的 `B`
    /// 落回 Ground 被当成普通字符上彩虹 —— 于是 SGR 插进了转义序列中间,
    /// 终端只认到 `ESC (`(非法设计ator) 后把剩下的 `[38;2;…m` 当文本打印。
    /// 这正是 `rscat -e fastfetch` 末尾出现 `[38;2;21;240;121m` 之类的成因:
    /// fastfetch 退出时发的 `\e(B\e)B`(恢复 G0/G1 字符集)被拆坏了。
    EscMid,
}

/// ESC + 中间字节(0x20..=0x2F),后面必然跟一个 0x30..=0x7E 的收尾字符。
/// 这类序列最长 3 字节(如 `ESC ( B`),不含参数,也没有超过 1 个收尾字符的形式。
#[inline]
fn is_esc_intermediate(b: u8) -> bool {
    (0x20..=0x2F).contains(&b)
}

pub struct LolcatFilter {
    out: Box<dyn Write>,
    freq: f64,
    spread: f64,
    os: f64,
    painter: Painter,
    animate: bool,
    duration: usize,
    speed: f64,
    state: State,
    run: Vec<u8>,
    str_esc: bool,
    txt: Vec<u8>,
    i: usize,
    run_emitted: bool,
    line: Vec<(Vec<u8>, Option<char>)>,
    saw_nl: bool,
    /// 输入流当前是否处于背景色中:背景 SGR(48;2/48;5/40-47/100-107)置位,
    /// 0/49 复位。块状图片/logo(fastfetch 等)用背景色画格子,处于背景色中的
    /// 配对原样直通(不被彩虹覆盖);前景色文本照常上彩虹。
    colored_bg: bool,
    /// 生产者(被过滤的子进程)是否设置了显式前景色:true = 后续字符自带颜色。
    /// 块元素字符(█▀▄▌▐░▒▓)在这个状态下视为"图片像素"——fastfetch/chafa 把
    /// 图片逐格画成这些字符,每格带自己的颜色——原样直通不上彩虹。
    /// 普通文字不管有没有颜色都照常上彩虹(含 ///// 这类 ASCII art logo)。
    producer_fg: bool,
    /// OSC 133 shell 集成区域跟踪:true = 提示符/命令行区域(不彩虹,
    /// 子进程自带的颜色原样保留),false = 命令输出区域(上彩虹)。
    /// 标记:A=提示符开始 B=命令行开始 C=输出开始 D=命令结束。
    suppress: bool,
}

impl LolcatFilter {
    // 8 个参数都是独立旋钮;包成结构体只是把参数搬个家,反而隔一层,故保留。
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        out: Box<dyn Write>,
        freq: f64,
        spread: f64,
        seed: u64,
        painter: Painter,
        animate: bool,
        duration: usize,
        speed: f64,
    ) -> LolcatFilter {
        LolcatFilter {
            out,
            freq,
            spread: spread.max(0.1),
            os: seed as f64 + 1.0,
            painter,
            animate,
            duration,
            speed,
            state: State::Ground,
            run: Vec::new(),
            str_esc: false,
            txt: Vec::new(),
            i: 0,
            run_emitted: false,
            line: Vec::new(),
            colored_bg: false,
            producer_fg: false,
            suppress: false,
            saw_nl: false,
        }
    }

    fn write_raw(&mut self, data: &[u8]) {
        let _ = self.out.write_all(data);
    }

    /// 一个配对:转义连跑(可空) + 单个字符(可空)。
    fn pair(&mut self, ch: Option<char>) {
        let mut run = Vec::new();
        if self.run_emitted {
            self.run_emitted = false; // 已即时写出,字节顺序仍与 lolcat 一致
        } else if !self.run.is_empty() {
            std::mem::swap(&mut run, &mut self.run);
        }
        if self.animate {
            self.line.push((run, ch));
            return;
        }
        self.write_pair(&run, ch, self.i, false);
        self.i += 1;
    }

    /// 转义序列完结即直通:查询/图片协议必须立刻到达终端,
    /// 否则子进程等应答、缓冲等字符,互相死锁到超时(animate 保持整行缓冲除外)。
    fn seq_done(&mut self) {
        if !self.run.is_empty() {
            // 先更新"处于背景色"状态再冲刷:run 末尾刚完成的序列可能置位/复位背景色
            let mut bg = self.colored_bg;
            let mut fg = self.producer_fg;
            scan_sgr_state(&self.run, &mut bg, &mut fg);
            self.colored_bg = bg;
            self.producer_fg = fg;
            if !self.animate {
                let run = std::mem::take(&mut self.run);
                self.write_raw(&run);
                self.run_emitted = true;
            }
        }
    }

    /// OSC 133 shell 集成标记:跟踪提示符/输出区域。
    /// A=提示符开始 B=命令行开始 C=输出开始 D=命令结束。
    /// A/B/D 区域不彩虹(保留子进程自带颜色),C 之后上彩虹。
    fn osc133_check(&mut self) {
        if self.run.len() >= 7 && self.run.starts_with(b"\x1b]133;") {
            match self.run[6] {
                b'C' => self.suppress = false,
                b'A' | b'B' | b'D' => self.suppress = true,
                _ => {}
            }
        }
    }

    fn write_pair(&mut self, run: &[u8], ch: Option<char>, idx: usize, frame_strip: bool) {
        // OSC 133 提示符/命令行区域:不上彩虹,子进程自带颜色原样保留。
        if self.suppress {
            if !run.is_empty() {
                self.write_raw(run);
            }
            if let Some(c) = ch {
                let mut b = [0u8; 4];
                self.write_raw(c.encode_utf8(&mut b).as_bytes());
            }
            return;
        }
        // 图片单元格原样直通,不被彩虹覆盖,也不补 \e[39m,保持字节保真:
        //   1) 背景色格子(colored_bg)——块状图片常用背景色画格;
        //   2) 带显式前景色的块元素字符(█▀▄▌▐░▒▓)——fastfetch/chafa 把图片
        //      逐格画成这些字符,每格自带颜色(如 Windows logo 的
        //      \e[38;2;0;120;212m█)。不保住它们,logo 会被彩虹糊掉,暗色格子
        //      还会在深色背景上"消失",看起来就像与右侧信息错位、图片缺块。
        // 纯文字(含 ///// 这类 ASCII art)不含块元素字符,照常上彩虹。
        if self.colored_bg || (self.producer_fg && ch.is_some_and(is_picture_cell)) {
            if frame_strip && run.contains(&0x1B) {
                let stripped = strip_animate_csi(run);
                self.write_raw(&stripped);
            } else if !run.is_empty() {
                self.write_raw(run);
            }
            if let Some(c) = ch {
                let mut b = [0u8; 4];
                self.write_raw(c.encode_utf8(&mut b).as_bytes());
            }
            return;
        }
        let run_bytes: &[u8] = if frame_strip {
            // animate 重绘帧:剥掉清行类 CSI(与 lol.rb println_ani 一致)。
            // 为避免分配,仅当包含 ESC 时才走剥离路径。
            if run.contains(&0x1B) {
                let stripped = strip_animate_csi(run);
                // write_pair 需要 &self.painter 可变借用,先把 stripped 存起来
                self.write_raw(&stripped);
                let (r, g, b) = rainbow(self.freq, self.os + idx as f64 / self.spread);
                let (sgr, reset) = self.painter.sgr(r, g, b);
                let sgr = sgr.to_vec();
                self.write_raw(&sgr);
                if let Some(c) = ch {
                    let mut b = [0u8; 4];
                    self.write_raw(c.encode_utf8(&mut b).as_bytes());
                }
                self.write_raw(reset);
                return;
            }
            run
        } else {
            run
        };
        if !run_bytes.is_empty() {
            self.write_raw(run_bytes);
        }
        let (r, g, b) = rainbow(self.freq, self.os + idx as f64 / self.spread);
        let (sgr, reset) = self.painter.sgr(r, g, b);
        let sgr = sgr.to_vec();
        self.write_raw(&sgr);
        if let Some(c) = ch {
            let mut b = [0u8; 4];
            self.write_raw(c.encode_utf8(&mut b).as_bytes());
        }
        self.write_raw(reset);
    }

    /// 行收尾:每行多出一个"空尾配对",换行后 os+=1。
    fn endline(&mut self) {
        self.pair(None);
        if self.animate {
            self.animate_line();
        }
        self.write_raw(b"\n");
        self.os += 1.0;
        self.i = 0;
        self.saw_nl = true;
    }

    fn animate_line(&mut self) {
        // println_ani:\e7 存光标 → duration 帧(\e8 回光标,os+=spread 重绘)。
        if self.line.is_empty() {
            return;
        }
        let real_os = self.os;
        self.write_raw(b"\x1b7");
        for frame in 0..self.duration {
            self.write_raw(b"\x1b8");
            self.os += self.spread;
            let line = std::mem::take(&mut self.line);
            for (idx, (run, ch)) in line.iter().enumerate() {
                self.write_pair(run, *ch, idx, frame > 0);
                let _ = self.out.flush();
            }
            self.line = Vec::new();
            std::thread::sleep(std::time::Duration::from_secs_f64(1.0 / self.speed));
        }
        self.os = real_os;
    }

    fn emit_char(&mut self, ch: char) {
        if ch == '\n' {
            self.endline();
        } else if ch == '\t' {
            for _ in 0..8 {
                // tab 展开成 8 个独立空格配对
                self.pair(Some(' '));
            }
        } else {
            self.pair(Some(ch));
        }
    }

    /// 增量 UTF-8 解码(语义同 Python errors="replace" 增量解码器):
    /// 完整字符落盘;非法字节变 U+FFFD;末尾残缺序列留待下批数据。
    fn flush_text(&mut self) {
        // 先把本轮可解码字符收集成 owned,再逐个 emit(避开借用冲突)。
        let mut chars: Vec<char> = Vec::new();
        let mut i = 0;
        while i < self.txt.len() {
            match std::str::from_utf8(&self.txt[i..]) {
                Ok(s) => {
                    chars.extend(s.chars());
                    i = self.txt.len();
                }
                Err(e) => {
                    let valid = e.valid_up_to();
                    if valid > 0 {
                        // valid_up_to 保证此前缀合法
                        let s = std::str::from_utf8(&self.txt[i..i + valid]).unwrap();
                        chars.extend(s.chars());
                        i += valid;
                    }
                    match e.error_len() {
                        Some(n) => {
                            chars.push('\u{FFFD}');
                            i += n;
                        }
                        None => break, // 残缺尾,等下批
                    }
                }
            }
        }
        self.txt.drain(..i);
        for ch in chars {
            self.emit_char(ch);
        }
    }

    /// 转义序列状态机(序列进 run,原样直通)。
    pub fn feed(&mut self, data: &[u8]) {
        for &byte in data {
            match self.state {
                State::Ground => {
                    if byte == 0x1B {
                        self.flush_text();
                        self.state = State::Esc;
                    } else {
                        self.txt.push(byte);
                    }
                }
                State::Esc => {
                    if byte == b'[' {
                        self.state = State::Csi;
                        self.run.extend_from_slice(b"\x1b[");
                    } else if byte == b']' {
                        self.state = State::Osc;
                        self.run.extend_from_slice(b"\x1b]");
                    } else if byte == 0x50 || byte == 0x58 || byte == 0x5E || byte == 0x5F {
                        // DCS/SOS/PM/APC(图片协议)
                        self.state = State::Str;
                        self.str_esc = false;
                        self.run.push(0x1B);
                        self.run.push(byte);
                    } else if is_esc_intermediate(byte) {
                        // ESC + 中间字节:可能是 3 字节转义(SCS 等),等收尾字符
                        self.state = State::EscMid;
                        self.run.push(0x1B);
                        self.run.push(byte);
                    } else {
                        self.state = State::Ground;
                        self.run.push(0x1B);
                        self.run.push(byte);
                        self.seq_done(); // 2 字符转义并入下一个配对
                    }
                }
                State::EscMid => {
                    self.run.push(byte);
                    // 收尾字符(0x30..=0x7E)到达,整个 3 字节序列完成。
                    // 若收到的仍是中间字节,继续留在本状态(标准允许连写多个,
                    // 例如 ESC ( ( B;实际终端极少见,但不该把中间字节当收尾)。
                    if (0x30..=0x7E).contains(&byte) {
                        self.state = State::Ground;
                        self.seq_done();
                    }
                }
                State::Csi => {
                    self.run.push(byte);
                    if (0x40..=0x7E).contains(&byte) {
                        self.state = State::Ground;
                        self.seq_done();
                    }
                }
                State::Osc => {
                    self.run.push(byte);
                    if byte == 0x07 {
                        self.state = State::Ground;
                        self.osc133_check();
                        self.seq_done();
                    } else if byte == 0x1B {
                        self.state = State::OscEsc;
                    }
                }
                State::OscEsc => {
                    self.run.push(byte);
                    if byte == b'\\' {
                        self.state = State::Ground;
                        self.osc133_check();
                        self.seq_done();
                    } else {
                        self.state = State::Osc;
                    }
                }
                State::Str => {
                    self.run.push(byte);
                    if self.str_esc {
                        if byte == b'\\' {
                            // ST:图片载荷结束
                            self.state = State::Ground;
                            self.seq_done();
                        } else {
                            self.str_esc = byte == 0x1B;
                        }
                    } else if byte == 0x1B {
                        self.str_esc = true;
                    }
                }
            }
        }
        // 立即吐出已可解码的文本:交互式会话(-a)里 shell 的提示符与内核
        // 对按键的回显都是"不含任何转义序列的纯文本",若一直攒着不吐,用户
        // 就看不到自己输入的内容,直到回车时 readline 发出转义序列才被
        // 顺带冲出(症状:输入不显示,按回车后才显示)。残缺的 UTF-8 尾仍
        // 留在缓冲里等下一批,故不会切坏多字节字符。
        self.flush_text();
        let _ = self.out.flush();
    }

    /// 原始字节直通(图片载荷用):先落盘挂起的文本保证顺序,再原样写入并刷盘。
    /// 不经过染色,图片协议不被破坏。
    pub fn feed_raw(&mut self, data: &[u8]) {
        self.flush_text();
        self.write_raw(data);
        let _ = self.out.flush();
    }

    pub fn finish(&mut self, stdout_is_tty: bool) {        self.flush_text();
        if !self.animate && self.state != State::Ground && !self.run.is_empty() {
            self.run.clear(); // 末尾残缺的转义序列:按 lolcat 行为丢弃
        }
        if !self.saw_nl {
            // 末行无换行符:补空尾配对 + os 推进
            self.pair(None);
            if self.animate {
                self.animate_line();
            }
            self.os += self.i as f64 / self.spread;
        }
        if stdout_is_tty {
            self.write_raw(b"\x1b[m\x1b[?25h\x1b[?1;5;2004l");
        }
        let _ = self.out.flush();
    }
}

/// 剥掉清行/清屏类 CSI(animate 重绘帧用,对应 lol.rb 的 gsub(/\e\[[0-?]*[@JKPX]/))。
fn strip_animate_csi(run: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(run.len());
    let mut i = 0;
    while i < run.len() {
        if run[i] == 0x1B && i + 1 < run.len() && run[i + 1] == b'[' {
            let mut j = i + 2;
            while j < run.len() && (0x30..=0x3F).contains(&run[j]) {
                j += 1;
            }
            if j < run.len() {
                let f = run[j];
                if f == b'@' || f == b'J' || f == b'K' || f == b'P' || f == b'X' {
                    i = j + 1; // 整个序列丢掉
                    continue;
                }
            }
        }
        out.push(run[i]);
        i += 1;
    }
    out
}

/// 从字符串开头剥掉一条 SGR 序列(\e[...m);不是 SGR 时返回 None。
#[cfg(test)]
fn strip_one_sgr(s: &str) -> Option<&str> {
    let b = s.as_bytes();
    if b.len() < 3 || b[0] != 0x1B || b[1] != b'[' {
        return None;
    }
    let mut j = 2;
    while j < b.len() && !(0x40..=0x7E).contains(&b[j]) {
        j += 1;
    }
    if j < b.len() && b[j] == b'm' {
        Some(&s[j + 1..])
    } else {
        None
    }
}

/// 块元素字符:fastfetch/chafa 等把图片逐格画成这些字符(每格自带颜色),
/// 而普通文字与 ASCII art(///// 等)不会用到它们。用于区分"图片像素"与文字。
fn is_picture_cell(c: char) -> bool {
    matches!(c,
        '█' | '▀' | '▄' | '▌' | '▐' | '░' | '▒' | '▓'
        | '▁' | '▂' | '▃' | '▅' | '▆' | '▇' | '▉' | '▊' | '▋' | '▍' | '▎' | '▏' | '▕' | '▔'
        | '▖' | '▗' | '▘' | '▙' | '▚' | '▛' | '▜' | '▝' | '▞' | '▟'
    )
}

/// 扫描一段转义序列(r 或多个连跑的序列),更新生产者颜色状态。
/// 只认 SGR(\e[...m):背景 48(2;r;g;b 与 5;n)及 40-47/100-107 置 bg,
/// 前景 38(2;r;g;b 与 5;n)及 30-37/90-97 置 fg;
/// 0 与空参数(\e[m)同时复位两者,39 复位前景,49 复位背景。
/// 38/48 的颜色分量必须整体跳过,否则 `48;2;0;120;212` 里的 0 会被误当复位。
fn scan_sgr_state(run: &[u8], bg: &mut bool, fg: &mut bool) {
    let mut i = 0;
    while i < run.len() {
        if run[i] != 0x1B || i + 1 >= run.len() || run[i + 1] != b'[' {
            i += 1;
            continue;
        }
        let start = i + 2;
        let mut j = start;
        while j < run.len() && !(0x40..=0x7E).contains(&run[j]) {
            j += 1;
        }
        if j >= run.len() {
            return; // 残缺序列(不会出现在 seq_done 的 run 里,防御用)
        }
        if run[j] == b'm' {
            apply_sgr(&run[start..j], bg, fg);
        }
        i = j + 1;
    }
}

/// 应用一条 SGR 参数串(`\e[` 与 `m` 之间的字节)到生产者颜色状态。
fn apply_sgr(params: &[u8], bg: &mut bool, fg: &mut bool) {
    if params.is_empty() {
        *bg = false;
        *fg = false;
        return;
    }
    let mut toks: Vec<u32> = Vec::with_capacity(8);
    let mut cur: u32 = 0;
    for &b in params {
        match b {
            b'0'..=b'9' => cur = cur.saturating_mul(10) + (b - b'0') as u32,
            b';' | b':' => {
                toks.push(cur); // 空参数按 0(复位)
                cur = 0;
            }
            _ => return, // 非数字参数,放弃解析(保持状态不变)
        }
    }
    toks.push(cur);
    let mut k = 0;
    while k < toks.len() {
        match toks[k] {
            0 => {
                *bg = false;
                *fg = false;
            }
            39 => *fg = false,
            49 => *bg = false,
            30..=37 | 90..=97 => *fg = true,
            40..=47 | 100..=107 => *bg = true,
            38 | 48 => {
                // 先记住这是前景(38)还是背景(48),再整体跳过颜色分量,
                // 否则 `48;2;0;120;212` 里的 0 会被误当复位。
                let is_fg = toks[k] == 38;
                match toks.get(k + 1) {
                    Some(2) => k += 4, // 2;r;g;b
                    Some(5) => k += 2, // 5;n
                    _ => {}
                }
                if is_fg {
                    *fg = true;
                } else {
                    *bg = true;
                }
            }
            _ => {}
        }
        k += 1;
    }}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rainbow::{ColorMode, Painter};
    use std::cell::RefCell;
    use std::io::Write;
    use std::rc::Rc;

    /// 共享内存 writer,便于在测试里取回 filter 输出。
    #[derive(Clone)]
    struct Shared(Rc<RefCell<Vec<u8>>>);
    impl Write for Shared {
        fn write(&mut self, d: &[u8]) -> std::io::Result<usize> {
            self.0.borrow_mut().extend_from_slice(d);
            Ok(d.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    fn make(seed: u64) -> (LolcatFilter, Rc<RefCell<Vec<u8>>>) {
        let sink = Rc::new(RefCell::new(Vec::new()));
        let filter = LolcatFilter::new(
            Box::new(Shared(sink.clone())),
            0.1,
            3.0,
            seed,
            Painter::new(ColorMode::C256, false),
            false,
            1,
            20.0,
        );
        (filter, sink)
    }

    #[test]
    fn sgr_state_vectors() {
        let mut bg = false;
        let mut fg = false;
        // truecolor 里的 0 不是复位
        scan_sgr_state(b"\x1b[48;2;0;120;212m", &mut bg, &mut fg);
        assert!(bg);
        assert!(!fg);
        // 256 色:39 是色号,不是背景复位
        scan_sgr_state(b"\x1b[48;5;39m", &mut bg, &mut fg);
        assert!(bg);
        // 基本背景色
        scan_sgr_state(b"\x1b[44m", &mut bg, &mut fg);
        assert!(bg);
        // 前景色:38;2;r;g;b 分量里的 0 不能当复位;前景置位、背景不受影响
        scan_sgr_state(b"\x1b[38;2;0;120;212m", &mut bg, &mut fg);
        assert!(fg);
        assert!(bg);
        scan_sgr_state(b"\x1b[31m", &mut bg, &mut fg);
        assert!(fg);
        // 0 / 空参数 / 49 / 39 复位
        scan_sgr_state(b"\x1b[0m", &mut bg, &mut fg);
        assert!(!bg && !fg);
        scan_sgr_state(b"\x1b[m", &mut bg, &mut fg);
        assert!(!bg && !fg);
        scan_sgr_state(b"\x1b[38;2;1;2;3m\x1b[49m", &mut bg, &mut fg);
        assert!(!bg);
        assert!(fg, "49 must reset background only");
        scan_sgr_state(b"\x1b[39m", &mut bg, &mut fg);
        assert!(!fg, "39 must reset foreground");
        // 光标序列不是 SGR,不影响状态
        scan_sgr_state(b"\x1b[2;3H\x1b[2J", &mut bg, &mut fg);
        assert!(!bg && !fg);
    }

    #[test]
    fn picture_cells_recognised() {
        for c in ['█', '▀', '▄', '▌', '▐', '░', '▒', '▓'] {
            assert!(is_picture_cell(c), "{c} must count as a picture cell");
        }
        // 普通文字与 ASCII art 不算图片格
        for c in ['/', 'O', 'S', 'W', 'i', 'n', '─', '├', '└', ' ', '2'] {
            assert!(!is_picture_cell(c), "{c} must not count as a picture cell");
        }
    }

    /// 用户的 fastfetch logo.raw:每格 `\e[38;2;0;120;212m█`(前景色画格)。
    /// 必须原样直通——旧实现只认背景色,把 logo 糊成了彩虹。
    #[test]
    fn fg_colored_block_logo_passes_through_verbatim() {
        let (mut f, sink) = make(7);
        // 真实 logo 行:每格自带前景色(与 logo.raw 一致)
        let cell = "\x1b[38;2;0;120;212m█";
        let mut logo = Vec::new();
        for _ in 0..4 {
            logo.extend_from_slice(cell.as_bytes());
        }
        logo.extend_from_slice(b"\x1b[0m\n");
        f.feed(&logo);
        f.finish(false);
        let out = sink.borrow().clone();
        let s = String::from_utf8_lossy(&out);

        // logo 的原始字节必须逐字节成为输出的前缀(原 SGR 紧贴块字符,中间没被塞彩虹)
        assert!(
            s.starts_with(&cell.repeat(4)),
            "logo must pass through verbatim; got {s:?}"
        );
        // 其余部分只允许是:logo 自带的 \e[0m、换行、以及 lolcat 语义的"空尾配对"
        // (空尾配对不落在任何可见字符上,无可见效果)。
        let rest = s[cell.repeat(4).len()..].replace("\x1b[0m", "").replace('\n', "");
        let mut stripped = rest.as_str();
        while let Some(stripped2) = strip_one_sgr(stripped) {
            stripped = stripped2;
        }
        assert!(
            stripped.is_empty(),
            "no visible character may be rainbowed inside the logo; leftover {stripped:?}"
        );
    }

    /// ASCII art logo(///// 之类,无自带颜色)仍要上彩虹。
    #[test]
    fn plain_ascii_art_still_rainbowed() {
        let (mut f, sink) = make(7);
        f.feed(b"//////////  /////////\n");
        f.finish(false);
        let out = sink.borrow().clone();
        assert!(
            out.windows(5).any(|w| w == b"38;5;") || out.windows(5).any(|w| w == b"38;2;"),
            "ASCII art without its own colour must still get the rainbow"
        );
    }

    /// 带前景色的普通文字(如 fastfetch 的彩色键名)照常上彩虹。
    #[test]
    fn fg_colored_text_still_rainbowed() {
        let (mut f, sink) = make(7);
        f.feed("\x1b[38;2;190;194;255mKEY value\n".as_bytes());
        f.finish(false);
        let out = sink.borrow().clone();
        let s = String::from_utf8_lossy(&out);
        assert!(
            out.windows(5).any(|w| w == b"38;5;") || out.windows(5).any(|w| w == b"38;2;"),
            "coloured text must be rainbowed"
        );
        assert!(s.contains('K') && s.contains('v'));
    }

    #[test]
    fn bg_colored_passthrough_keeps_original_bytes() {
        let (mut f, sink) = make(7);
        let input = b"\x1b[38;2;0;120;212m\x1b[48;2;0;120;212m\x1b[38;5;198mAB\n";
        f.feed(input);
        f.finish(false);
        let out = sink.borrow().clone();
        let s = String::from_utf8_lossy(&out);
        // 背景色格子原样直通:原 SGR 与字符紧邻,不插入 38;5 彩虹,也不补 \e[39m
        assert!(s.contains("\x1b[38;2;0;120;212m\x1b[48;2;0;120;212m\x1b[38;5;198mAB"),
            "bg-colored segment must pass through verbatim, got {s:?}");
        assert!(!out.windows(4).any(|w| w == b"\x1b[39m"), "no extra resets in bg-colored region");
    }

    #[test]
    fn plain_text_still_rainbowed() {
        let (mut f, sink) = make(7);
        f.feed(b"hello\n");
        f.finish(false);
        let out = sink.borrow().clone();
        assert!(out.windows(5).any(|w| w == b"38;5;"), "plain text must be rainbowed");
        assert!(out.contains(&b'h'));
    }

    #[test]
    fn bg_then_plain_mixed() {
        let (mut f, sink) = make(7);
        // 前半行处于背景色(直通),\e[0m 复位后的后半行无色(上彩虹)
        f.feed(b"\x1b[48;2;10;20;30mBLOCK\x1b[0m plain\n");
        f.finish(false);
        let s = String::from_utf8_lossy(&sink.borrow()).to_string();
        let block_at = s.find("BLOCK").expect("BLOCK present");
        // BLOCK 直通:紧贴原 SGR,且 BLOCK 后紧跟原始的 \e[0m(中间无任何插入)
        assert!(s[..block_at].ends_with("\x1b[48;2;10;20;30m"));
        assert!(s[block_at + 5..].starts_with("\x1b[0m"));
        // 复位之后的文本恢复彩虹(第一个字符起就是 38;5;N)
        let reset_at = block_at + 5 + 4; // "\x1b[0m" 长 4
        assert!(s[reset_at..].starts_with("\x1b[38;5;"));
    }

    /// -a 彩虹会话的核心场景:shell 发完提示符后,用户逐键输入的字符由内核回显,
    /// 这中间可以长时间没有任何转义序列。文本必须立刻可见——否则就是
    /// 「输入不显示,按回车后才显示」(回车时 readline 才发转义序列,
    /// 攒下的提示符+整行输入被一次性吐出)。
    #[test]
    fn interactive_text_visible_before_any_escape() {
        let (mut f, sink) = make(7);
        // 会话 shell 先发提示符区域标记(此后提示符区域原样直通)
        f.feed(b"\x1b]133;A\x07");
        f.feed(b"[user@host ~]$ ");
        let after_prompt = String::from_utf8_lossy(&sink.borrow()).to_string();
        assert!(
            after_prompt.contains("[user@host ~]$ "),
            "prompt must be visible immediately, got {after_prompt:?}"
        );

        // 用户按键 e、c:内核回显,同样不含任何转义序列
        f.feed(b"ec");
        let after_keys = String::from_utf8_lossy(&sink.borrow()).to_string();
        assert!(
            after_keys.contains("[user@host ~]$ ec"),
            "typed keys must be visible before Enter, got {after_keys:?}"
        );
    }

    /// OSC 133 区域跟踪:A/B/D(提示符/命令行)不彩虹,C 之后(命令输出)上彩虹。
    #[test]
    fn osc133_regions_track_prompt_and_output() {
        let (mut f, sink) = make(7);
        f.feed(b"\x1b]133;A\x07");
        f.feed(b"prompt$ ");
        let s1 = String::from_utf8_lossy(&sink.borrow()).to_string();
        assert!(
            s1.contains("prompt$ "),
            "prompt region must pass through verbatim, got {s1:?}"
        );
        assert!(
            !s1.contains("\x1b[38;5;"),
            "prompt region must not be rainbowed, got {s1:?}"
        );

        // C=输出开始:之后的文本上彩虹
        f.feed(b"\x1b]133;C\x07");
        f.feed(b"out\n");
        let s2 = String::from_utf8_lossy(&sink.borrow()).to_string();
        assert!(
            s2.contains("\x1b[38;5;"),
            "command output must be rainbowed after OSC 133;C, got {s2:?}"
        );

        // D=命令结束:回到不彩虹区域
        let before_d = sink.borrow().len();
        f.feed(b"\x1b]133;D\x07next$ ");
        let s3 = String::from_utf8_lossy(&sink.borrow()[before_d..]).to_string();
        assert!(
            s3.contains("next$ "),
            "prompt after OSC 133;D must pass through, got {s3:?}"
        );
        assert!(
            !s3.contains("\x1b[38;5;"),
            "region after OSC 133;D must not be rainbowed, got {s3:?}"
        );
    }

    /// 立即吐出文本不能把跨批次的 UTF-8 字符切坏:完整字符立即可见,
    /// 残缺尾仍留在缓冲里等下一批(不得变成 U+FFFD)。
    #[test]
    fn complete_chars_flush_immediately_but_split_utf8_tail_is_held() {
        let (mut f, sink) = make(7);
        let bytes = "中文".as_bytes();
        f.feed(&bytes[..4]); // 第一个字完整 + 第二个字的首字节
        let first = String::from_utf8_lossy(&sink.borrow()).to_string();
        assert!(
            first.contains('中'),
            "complete char must flush at once, got {first:?}"
        );
        assert!(
            !first.contains('\u{FFFD}'),
            "a split UTF-8 tail must not become U+FFFD, got {first:?}"
        );

        f.feed(&bytes[4..]);
        f.finish(false);
        let second = String::from_utf8_lossy(&sink.borrow()).to_string();
        assert!(
            second.contains('文'),
            "split tail must complete, got {second:?}"
        );
        assert!(
            !second.contains('\u{FFFD}'),
            "no U+FFFD anywhere, got {second:?}"
        );
    }

    /// `rscat -e fastfetch` 末尾出现神秘文字(`[38;2;21;240;121m` 之类)的回归测试。
    ///
    /// fastfetch 退出时会发 `\e(B\e)B` 恢复 G0/G1 字符集。旧状态机把 `ESC ( B`
    /// 当成"两字符转义"在 `(` 处就结束,剩下的 `B` 落回 Ground 被当普通字符上色 ——
    /// SGR 于是插进了转义序列中间,终端只认到 `ESC (` 这个非法 designator,
    /// 把随后的 `[38;2;…m` 当文本打印出来。
    #[test]
    fn scs_charset_escapes_pass_through_intact() {
        let (mut f, sink) = make(42);
        f.feed(b"X\x1b(B\x1b)BY\n");
        f.finish(false);
        let out = sink.borrow().clone();
        assert!(
            out.windows(3).any(|w| w == b"\x1b(B"),
            "ESC ( B must survive verbatim, got {:?}",
            String::from_utf8_lossy(&out)
        );
        assert!(
            out.windows(3).any(|w| w == b"\x1b)B"),
            "ESC ) B must survive verbatim, got {:?}",
            String::from_utf8_lossy(&out)
        );
        // 中间不得插入任何 SGR:出现 `ESC ( ESC [` 就是 bug 复发
        assert!(
            !out.windows(4).any(|w| w == b"\x1b(\x1b["),
            "no SGR may be injected between ESC ( and its designator"
        );
    }

    /// 3 字节转义后面紧跟普通文本时,文本照常上彩虹(修复不能把文本也吞掉)。
    #[test]
    fn text_after_scs_still_gets_rainbowed() {
        let (mut f, sink) = make(11);
        f.feed(b"\x1b(Bhello\n");
        f.finish(false);
        let out = sink.borrow().clone();
        let s = String::from_utf8_lossy(&out);
        assert!(s.contains('h'), "text after the SCS escape must still be emitted: {s:?}");
        assert!(
            out.windows(2).any(|w| w == b"38"),
            "the trailing text must still be rainbow-coloured"
        );
    }

    /// `fastfetch | rscat` / `rscat -e fastfetch` 图片不显示的回归测试。
    ///
    /// kitty 图形协议用 APC 封装:`ESC _ <payload> ST`(`ESC _` = `0x1b 0x5f`),
    /// 多块传输时 payload 形如 `f=32,a=T;...`(`a=T` 首块)与 `m=32,a=T;...`
    /// (`m` 续块),以 `ESC \` 结束。过滤器必须把整条 APC 原样直通,不能:
    ///   * 把 `ESC _` 当普通转义在 `_` 处断开(那样 payload 会被拆去上彩虹,
    ///     终端只认到半个 APC,图片整条作废);
    ///   * 在 APC 里插入任何彩虹 SGR。
    #[test]
    fn kitty_graphics_apc_passes_through_verbatim() {
        let (mut f, sink) = make(42);
        let apc = b"\x1b_Gf=32,a=T;/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAAgGBgcGBQgHBwcJCQgKDBQNDAsLDBkSEw8UHRofHh0aHBwgJC4nICIsIxwcKDcpLDAxNDQ0Hyc5PTgyPC4zNDL/wAALCAABAAEBAREA/8QAFAABAAAAAAAAAAAAAAAAAAAAAP/EABQQAQAAAAAAAAAAAAAAAAAAAAD/xAAUAQEAAAAAAAAAAAAAAAAAAAAA/8QAFBEBAAAAAAAAAAAAAAAAAAAAAP/aAAwDAQACEQMRAD8AvwD/2Q==\x1b\\";
        f.feed(apc);
        f.finish(false);
        let out = sink.borrow().clone();
        // 整条 APC 必须原样出现在输出里
        assert!(
            out.windows(apc.len()).any(|w| w == apc),
            "kitty graphics APC must pass through byte-for-byte, got {:?}",
            String::from_utf8_lossy(&out)
        );
        // APC 内部不得插入任何 38;2 / 38;5 彩虹 SGR
        assert!(
            !out.windows(6).any(|w| w == b"\x1b[38;"),
            "no rainbow SGR may be injected inside the kitty APC"
        );
        // 不允许出现被拆坏的 APC:`ESC _` 之后不应紧跟 `ESC [`(说明序列被截断)
        assert!(
            !out.windows(4).any(|w| w == b"\x1b_\x1b["),
            "kitty APC must not be split (ESC _ ... ESC [ would mean a break)"
        );
    }

    /// 多块 kitty 图形传输(首块 `f=` 之后接续块 `m=`)逐块都须原样直通。
    #[test]
    fn kitty_graphics_multi_chunk_passthrough() {
        let (mut f, sink) = make(7);
        let chunk1 = b"\x1b_Gf=32,a=T;QUJD\x1b\\";
        let chunk2 = b"\x1b_Gm=32,a=T;REVGRw==\x1b\\";
        f.feed(chunk1);
        f.feed(chunk2);
        f.finish(false);
        let out = sink.borrow().clone();
        assert!(
            out.windows(chunk1.len()).any(|w| w == chunk1),
            "first kitty chunk must survive verbatim"
        );
        assert!(
            out.windows(chunk2.len()).any(|w| w == chunk2),
            "continuation kitty chunk must survive verbatim"
        );
        // 两个 APC 必须原样存在(末尾可能附带 lolcat 风格的空尾配对 SGR,不计其数)。
        assert!(out.windows(chunk1.len()).any(|w| w == chunk1));
        assert!(out.windows(chunk2.len()).any(|w| w == chunk2));
        // 任一 APC 内部都不得被插入彩虹 SGR
        for apc in [chunk1.as_slice(), chunk2.as_slice()] {
            let body = &apc[..apc.len() - 2]; // 去掉结尾 ST(ESC \)
            let mut needle = body.to_vec();
            needle.extend_from_slice(b"\x1b[38;");
            assert!(
                !out.windows(needle.len()).any(|w| w == needle.as_slice()),
                "no rainbow SGR may be injected inside a kitty APC"
            );
        }
    }
}
