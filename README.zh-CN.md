# rscat 🌈🐱

[**English**](README.md) | [**简体中文**](README.zh-CN.md)

![](ass/nom.jpg)

## 这是什么?

`rscat` 让 [lolcat](https://github.com/busyloop/lolcat) **支持图片输出**,并且是一个
**跨平台**工具。

lolcat 只把文字彩虹化,完全不认识图片。把**任何**能输出图片的工具接给它——系统信息
fetch、图片查看器、绘图脚本、预览工具——图就毁了:图形序列可能根本过不了管道,
侥幸到达的部分还会被逐格涂满。`rscat` 让彩虹与图片**共存于同一份输出**,
于是任何会画图的东西都能安全地彩虹化。

- **彩虹为本**:上色与 lolcat 100.0.1 逐字节一致——同样的选项、同样的输出,可直接替换。
- **认识图片**:图形协议与图片单元格原样直通,不被破坏。
- **跨平台**:一套 Rust 代码覆盖 Linux、macOS、FreeBSD、Windows——见[平台矩阵](#平台支持)。

它是早期单文件 Python 工具 `nyacat` 的 Rust 重写版。

## 为什么需要 rscat

彩虹工具把终端的输出流当成纯文本。可任何**画像素**的东西——kitty 图形协议、sixel、
iTerm2 内联图片,或者用 `█▀▄▌▐░▒▓` 这类块元素字符在文本里"拼"出来的图——要么被搅乱,
要么被涂成一团乱码。对 `cat` 无所谓,对 logo 是致命的。

`rscat` 建立在相反的假设上:它足够理解转义流,能分清**像素与文字**,只给文字上色。

## 效果

只要一个工具同时输出图片和文字,差别立刻就出来了。任何往终端画图的东西都可以:

```bash
# 带 logo 的系统信息 fetch
fastfetch --pipe false | rscat
neofetch | rscat

# 图片查看与预览
chafa photo.png | rscat
viu artwork.jpg | rscat

# 任何绘图/渲染,包括你自己的脚本
my-plot-script.py | rscat
```

不用 rscat,图会被涂成乱码或整个丢掉;用了 rscat,图片留下,文字是彩虹。

三种用法:

```bash
rscat logo.png          # 图片模式:直接画出 PNG/JPEG/GIF/BMP/WebP
some-command | rscat    # 过滤模式:彩虹化输出流,图片不被破坏
rscat -e fastfetch      # 运行模式:在伪终端里跑命令,图片与彩虹兼得
```

> **管道的一个前提。** 有些工具会先检查 stdout 是不是终端,非终端时干脆不输出图片。
> fastfetch 是常见例子。**装包就自动接好了**(见「安装」);从源码用时 source 一次
> `rscat --init` 也会处理好:片段里定义了一个同名 `fastfetch` 包装函数,
> **只在管道读端确实是 `rscat` 时**才追加 `--pipe false`:
>
> ```bash
> fastfetch | rscat        # 图片 + 彩虹字,不用手加旗标
> fastfetch | lolcat       # 不干预 —— 读端不是 rscat
> fastfetch > out.txt      # 不会把图片转义码写进文件
> fastfetch                # 原样调用(fastfetch 在终端里本来就出图)
> ```
>
> 这个判定刻意做得很窄:**不会**仅仅因为"stdout 是管道"就追加
> `--pipe false`(那会改变 `fastfetch | lolcat`、`fastfetch | grep` 的行为)。
> 包装函数对 bash / zsh / sh / fish 都提供。要绕过它用 `command fastfetch`;
> 自己显式传了 `--pipe` 就不会再追加。若是别的工具在管道里藏图、又没有这类旗标,
> 改用 `rscat -e <工具>`,它会拿到一个真正的伪终端。

## 特性

| | |
|---|---|
| **与 lolcat 一致的彩虹** | 与 lolcat 100.0.1 逐字节一致(8 组参考向量对照通过,真彩与 256 色均验证)。默认真彩,因此渐变逐字符平滑,而不是一块一块的色带。 |
| **图片直通** | kitty 图形协议 / sixel / iTerm2 OSC 序列原样转发,图片不被破坏。 |
| **图片单元保持原样** | 自带颜色的块元素字符——`fastfetch`、`chafa` 等在文本终端里画图的方式——原样直通。普通文字(含 ASCII art logo)照常上彩虹。 |
| **跨平台** | 一套代码,四个平台:Linux、macOS、FreeBSD、Windows,各有原生安装包。 |
| **运行模式** | `rscat -e <命令>`——在伪终端里跑任何东西,**图片与彩虹字兼得**,无需包装脚本、无需每个工具单独加旗标。 |
| **图片模式** | `rscat logo.png`——直接显示本地图片(PNG/JPEG/GIF/BMP/WebP)。 |
| **彩虹会话** | `rscat -a` 之后所有命令都是彩虹输出;`rscat -c` 取消。 |
| **多语言** | 英文 / 简体中文 / 繁体中文 / 日文,跟随系统 `LANG`,也可 `--lang` 指定。 |
| **调用 shell 检测** | `-e`/`-a` 能从父进程链里找到真实 shell,fetch 工具的 SHELL 模块显示 fish/zsh/bash 本尊而非 `rscat`。 |
| **六种 shell** | bash、zsh、sh、fish、PowerShell、cmd 的集成片段。 |

## 平台支持

| 平台 | 过滤 | 图片 | `-e` 运行 | `-a` 会话 | 说明 |
|---|---|---|---|---|---|
| Linux | ✅ 已测 | ✅ 已测 | ✅ 已测 | ✅ 已测 | 主力开发平台 |
| FreeBSD | ✅ 预期 | ✅ 预期 | ✅ 预期 | ✅ 预期 | 已提供包(基线 14.4,amd64/arm64),待实机验证 |
| macOS | ✅ 预期 | ✅ 预期 | ✅ 预期 | ✅ 预期 | 同上;iTerm2/WezTerm 用 `--proto iterm` |
| Windows | ✅ 已测 | ✅ 已测(WezTerm + `--proto iterm`) | ❌ 明确报错 | ❌ 明确报错 | `-e`/`-a` 需要 ConPTY,后续版本支持;rscat 会明确报错而不是刷乱码 |

“预期”= 代码只用了这些平台通用的 POSIX 接口(经 `libc`),但尚未在实机验证,欢迎报 issue。

## 安装

各平台的预编译二进制都发布在 [**Releases 页面**](../../releases),按平台下载:

| 平台 | 架构 | 文件 |
|---|---|---|
| Linux | x86_64 | `rscat-1.1.7-3-x86_64.pkg.tar.zst` / `rscat_1.1.7-3_amd64.deb` / `rscat-1.1.7-3.x86_64.rpm` |
| Linux | aarch64 | `rscat-1.1.7-3-aarch64.pkg.tar.zst` / `rscat-1.1.7-3_arm64.deb` / `rscat-1.1.7-3.aarch64.rpm` |
| FreeBSD | amd64 | `rscat-1.1.7-3-freebsd-amd64.pkg` |
| FreeBSD | arm64 | `rscat-1.1.7-3-freebsd-arm64.pkg` |
| macOS | arm64 | `rscat-1.1.7-3-macos-aarch64.pkg` |
| macOS | x86_64 | `rscat-1.1.7-3-macos-x86_64.pkg` |
| Windows | x86_64 | `rscat-1.1.7-3-windows-x86_64-setup.exe`(安装器)/ `.msi` |
| Windows | aarch64 | `rscat-1.1.7-3-windows-aarch64.zip`(绿色版) |

### Linux

```bash
sudo pacman -U rscat-1.1.7-3-x86_64.pkg.tar.zst    # Arch / CachyOS
sudo apt install ./rscat_1.1.7-3_amd64.deb        # Debian / Ubuntu
sudo rpm -Uvh rscat-1.1.7-3.x86_64.rpm            # Fedora / openSUSE
```

### FreeBSD

```bash
pkg add ./rscat-1.1.7-3-freebsd-amd64.pkg
```

### macOS

```bash
sudo installer -pkg rscat-1.1.7-3-macos-aarch64.pkg -target /   # Apple Silicon
sudo installer -pkg rscat-1.1.7-3-macos-x86_64.pkg  -target /   # Intel
```

### Windows

双击运行安装器,或在终端里装 MSI:

```powershell
.\rscat-1.1.7-3-windows-x86_64-setup.exe        # Inno Setup 安装器
msiexec /i rscat-1.1.7-3-windows-x86_64.msi     # WiX MSI
```

两者都会把 `rscat` 装到 `Program Files\rscat`,加入用户 `PATH`(卸载时移除),
并把 shell 片段放在 `shells\`。ARM64 的 zip 是绿色版,解压即可运行 `rscat.exe`。

### Linux、FreeBSD、macOS 安装时会自动接线 shell 集成

Linux、FreeBSD 与 macOS 的包都带有 post-install 钩子,会把片段接到你的各个 shell 上,
所以装完 `fastfetch | rscat` 立刻就有图 —— 不需要手动跑 `--init`:

| 落点 | Linux | FreeBSD | macOS | 覆盖的 shell |
|---|:--:|:--:|:--:|---|
| `~/.bashrc` | ✅ | ✅ | ✅ | 交互式**非登录** bash |
| `~/.bash_profile` | — | — | ✅ | macOS 的*登录* bash(它从不读 `~/.bashrc`) |
| `~/.zshrc` | ✅ | ✅ | ✅ | 交互式 zsh |
| `~/.profile` | ✅ | ✅ | ✅ | 登录 `sh`,以及没有 `~/.bash_profile` 的登录 bash |
| `~/.shrc` | ✅ | ✅ | — | 交互式**非登录** `sh`(Linux 的 `sh` 读 `$ENV`;macOS 的不读) |
| `~/.config/fish/conf.d/rscat.fish` | ✅ | ✅ | ✅ | fish —— 自动加载,无需改 rc |
| `/etc/profile.d/rscat.sh` | ✅ | ✅ | — | 所有账号的登录 shell,包括之后新建的账号 |
| `/etc/zshenv` | — | — | ✅ | macOS 系统级,`sudo -i` 这类 shell 也能覆盖 |
| fish 的 vendor/site `conf.d` | ✅ | ✅ | ✅ | 系统级 fish |

它遵守的规则:绝不覆盖你的 rc —— 只追加一个带围栏的块
(`# >>> rscat shell integration >>>` … `# <<< … <<<`),你可以手工删掉;
首次修改前留一份 `<rc>.rscat-bak`;幂等,重装不会堆叠;每步都非致命,
钩子失败绝不会让包事务失败。卸载时会把块和 drop-in 文件一并清掉。

当某个 shell 是账号的登录 shell 而对应 rc 不存在时,它还会**创建**该 rc
(`~/.zshrc`、`~/.bash_profile`、`~/.profile`),所以全新机器也能直接生效。

片段是**按引用**被 source 的
(`[ -r /usr/share/rscat/shells/rscat.bash ] && . …`),
所以升级包就会更新集成,不用改你的 rc 文件。
(例外:fish 的 `conf.d` 片段是**拷贝**而非引用。)

Windows 安装器会附带片段但**不会**改动 rc —— 那里请自行 source(见下节)。

### 从源码构建

```bash
cargo build --release
install -Dm755 target/release/rscat ~/.local/bin/rscat
```

可选的 shell 集成(幂等;会配好 `PATH` 以及上面说的 `fastfetch` 包装):

```bash
rscat --init fish >> ~/.config/fish/config.fish   # fish
rscat --init bash >> ~/.bashrc                    # bash
rscat --init zsh  >> ~/.zshrc                     # zsh
rscat --init sh   >> ~/.shrc                      # sh(交互式 sh 读 $ENV)
rscat --init powershell >> $PROFILE               # PowerShell
```

> **关于构建产物。** 对外发布的二进制与安装包挂在
> [GitHub Release](../../releases) 上。整个 `build/` 目录都被 git 忽略 ——
> 打包脚本、交接文档、二进制、安装包都只保存在本地。

## 用法

```bash

# 1 运行模式:在伪终端里跑任何命令,图片与彩虹字兼得
rscat -e fastfetch

# 2 图片模式:直接显示图片
rscat logo.png

# 3 彩虹会话:之后所有输出都是彩虹
rscat -a
rscat -c     # 或输入 exit
```

选项与 lolcat 相同(`-p/-F/-S/--animate/-d/-s/-i/-t/-f`),另加 `-e/--exec`、
`-a/--always`、`-c/--cancel`、`--proto`、`--image`、`--init`、`--lang`。
完整多语言帮助:`rscat -h`。

### 管道时"藏图"的工具

部分工具会检查 stdout 是否终端,非终端就跳过图片输出。fastfetch 是常见例子,
需要加 `--pipe false`（已做兼容处理）

```bash
fastfetch --pipe false | rscat
```

若该工具没有这类旗标,或者你不想逐个记,改用运行模式即可。`rscat -e` 给命令一个
真正的伪终端,它自己的终端检测就能通过,正常画图:

```bash
rscat -e chafa photo.png
rscat -e fastfetch
```

想把纯净输出重定向到文件,就别接 rscat(`command fastfetch`,或直接跑该工具)。

## 工作原理

`rscat` 是解析转义流,而不是把它当字节处理。有两类东西被识别为**非文字**,原样转发:

- **图形协议**——kitty APC、sixel DCS、iTerm2 OSC 1337。序列一结束就立刻写出,
  这样等待终端应答的程序不会死锁。
- **图片单元格**——自带颜色的块元素字符(`█▀▄▌▐░▒▓` 等)。`fastfetch`、`chafa` 就是
  用纯文本这样渲染图片的:一格代表一个像素,每格带自己的颜色。这类格子连同原色一起放行。

其余一切——包括用 `/`、`#` 或制表符拼成的 ASCII art logo——都是普通文字,照常上彩虹。
正是这条界线,让任何绘图工具都能在一次输出里同时给出图片和彩虹字,且与平台无关、
也不需要对那个工具做任何特别适配。

## 与前身 nyacat(Python 版)的差异

1. **修了图片/文字错位 bug**,分两层。(a) termios:运行模式曾对子伪终端和用户终端
   都用 `setraw()`,关掉 `ONLCR` 后裸 `\n` 不回车,输出呈楼梯状散架;现在子端保持
   cooked,只关 `ECHO`。(b) 协议层(真凶):`kitty-direct` 传的是*文件路径*,fastfetch
   必须发 `\x1b[6n` 查光标才知道图片占几行,但它只等约 50–100 ms,经代理的应答稍慢
   就超时,文字掉到图片下方。改用 `kitty` 类型后传内联 RGBA+zlib,排版用确定性的
   光标上移序列:零查询、零竞态。
2. **修了 `--animate` 必崩 bug**——Python 版调用 `bytearray.append(tuple)`,必然
   `TypeError`。Rust 版正确实现。
3. **`-a` 会话 / `-c` 取消是新增功能**;`-e` 取代了 Python 版的 `-c`。
4. **非 PNG 转码**由 Pillow 换成 `image` crate(纯 Rust,无系统依赖)。重采样器不同
   导致像素均值差约 2%(肉眼不可辨);尺寸与协议帧完全一致。
5. **调用 shell 检测**——fastfetch 按父进程报告 SHELL,直接 spawn 会显示 `rscat`。
   现在 rscat 沿内核父进程链找到真实 shell,并用它包一层。
6. **zsh 首次向导压制**——没有 `~/.zshrc` 时 `zsh-newuser-install` 会刷屏;
   现用临时 `ZDOTDIR`(含最小 `.zshrc`)压掉。
7. **TER/TFO 模块显示 `rscat` 或消失**——fastfetch 的终端模块取第一个非 shell 祖先进程
   当终端,rscat 正好卡位。现代理期间把 `comm` 临时伪装成调用 shell,让 fastfetch
   穿过它命中真实终端。
8. **终端标题与字体状态**在 `-e`/`-a` 退出后还原。
9. **fish 下 `-a` 会话里回车能执行了。** 代理读真终端时没清 `ICRNL`,内核会在转发前
   把用户的 `CR` 改写成 `LF`。bash/zsh 两者都接受,但 fish 自己关掉 ICANON 做行编辑,
   只认 `CR`=执行、`LF`=插入换行 —— 于是回车永远只是换行。现在 `stdin_cbreak()`
   同时清 `ICRNL`,让原始 `CR` 过桥;子 pty 从端自身的 `ICRNL` 仍会为未开 raw 模式的
   程序做转换,行为不变。
10. **终端能力查询由代理自己代答。** fish 4.x 会问终端是否支持 kitty 键盘协议
    (`CSI ? u`)。放行给真 kitty 会得到"支持",但 rscat 只原样转发按键、永不重编码成
    `CSI-u`,fish 于是等一个永远等不来的按键事件。现在 rscat 截下这一条查询并回
    `CSI ? 0 u`。**刻意不碰** XTVERSION 与 DECRQM —— 那些真终端能正确回答,
    代理代答反而给错信息。
11. **管道出图在装包时自动接好,且只在该接的时候接。** `fastfetch` 包装函数只在
    管道读端确实是 `rscat` 时才追加 `--pipe false` —— 判据是把当前 stdout 的
    `pipe:[inode]` 与各进程的 `fd/0` 比对(fish 则改为扫主进程的子进程,因为 fish
    把管道里的函数跑在主进程里、输出走内部缓冲)。早期版本改用进程组判断,那在启用任务
    控制的真实终端里必然失效:shell 给每条管道分配独立进程组,而管道函数里的 `$$`
    仍指向主 shell。

## 开发

```bash
cargo test          # 28 个单元 + 5 个 CLI 测试:彩虹向量、base64、PNG 头、魔数、
                    # 过滤器、PTY(termios + 终端查询代答)、BMP/WebP 解码、
                    # 会话标记、端到端 CLI
cargo build --release
```

```
src/            main.rs(CLI) rainbow.rs filter.rs image.rs shell_detect.rs
                pty_unix.rs pty_windows.rs persist.rs i18n.rs shell.rs
                help_{en,zh_cn,zh_tw,ja}.txt
shells/         rscat.{bash,zsh,sh,fish,ps1,cmd} —— --init 的正本,也是
                Linux/FreeBSD 包在安装时接进你 shell 的内容
tests/          cli.rs —— 运行编译后二进制的端到端测试
build/          打包配方 + 各平台交接文档(git 忽略)
ass/            图片素材
```

平台相关代码是隔离的:`pty_unix.rs` / `pty_windows.rs`,以及 `persist.rs` 里的
`cfg(unix)` / `cfg(windows)` 分支对。其余部分——过滤器、彩虹引擎、CLI——全部共享,
所以在颜色路径上的一次修复,对所有平台同时生效。

### shell 集成住哪

两层,改任一层时记得同步:

| 层 | 文件 | 作用 |
|---|---|---|
| 片段**正文** | `shells/rscat.*` | 定义 `PATH` 设置、`fastfetch` 包装,以及给包用的可 source 内容。通过 `include_str!` 编进二进制,所以 `rscat --init <shell>` 输出的就是这些文件本身。 |
| 接进用户 shell | `build/linux/scripts/post-install`、`build/linux/scripts/pre-remove`、`build/macos/scripts/postinstall`、`build/rebuild-freebsd-packages.sh` | 由包管理器调用,把片段从各 shell 正确的 rc 文件里 source 进来,并在卸载时撤销。 |

因为片段是**按引用**被 source 的,改片段文件 + 重建包,就足以更新所有用户 ——
不需要重写 rc。

## 致谢

猫图(`ass/nom.jpg`)来自 [lolcat](https://github.com/busyloop/lolcat),作者
[moe@busyloop.net](mailto:moe@busyloop.net)——本项目的兼容对象,彩虹也拜它所赐。
彩虹算法是对 lolcat 的 Rust 移植,已核对为逐字节一致的色彩输出。

## 许可

[MIT](LICENSE) © 2026 macOS Terminal。从 lolcat 移植的色彩代码与随附的猫图,
仍按 lolcat 的 **BSD-3-Clause** 授权。

许可原文、链接的 crate 列表,以及再分发时需要随附的内容,见
[`THIRD-PARTY.md`](THIRD-PARTY.md)。

每个安装包都会带上这两份文件,所以二进制安装本身就携带了 BSD-3 要求的声明:
`/usr/share/licenses/rscat/`(Arch、RPM)、`/usr/share/doc/rscat/`(Debian)、
`/usr/local/share/licenses/rscat/`(FreeBSD)、`/usr/local/share/doc/rscat/`(macOS)。
