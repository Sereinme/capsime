<div align="center">
  <img src="assets/icon/capsime-icon.png" width="132" alt="capsime" />
  <h1>capsime</h1>
  <p><b>在 Windows 上用 CapsLock 一键切换中英文输入法</b><br/>
  <sub>单个 exe · 零依赖 · 托盘常驻 · 不联网 · 不需要管理员权限</sub></p>
  <p>中文 · <a href="README.en.md">English</a></p>
  <a href="https://github.com/Sereinme/capsime/releases"><img alt="release" src="https://img.shields.io/github/v/release/Sereinme/capsime?style=flat-square&amp;sort=semver"></a>
  <a href="https://github.com/Sereinme/capsime/releases"><img alt="downloads" src="https://img.shields.io/github/downloads/Sereinme/capsime/total?style=flat-square"></a>
  <a href="https://github.com/Sereinme/capsime/blob/master/LICENSE"><img alt="license" src="https://img.shields.io/github/license/Sereinme/capsime?style=flat-square"></a>
  <a href="#"><img alt="platform" src="https://img.shields.io/badge/platform-Windows%2010%2F11%20x64-0078D6?style=flat-square"></a>
  <a href="#"><img alt="rust" src="https://img.shields.io/badge/rust-1.85%2B-orange?style=flat-square"></a>
  <a href="#"><img alt="size" src="https://img.shields.io/badge/exe-~350%20KB-6272A4?style=flat-square"></a>
</div>

CapsLock 常年吃灰。capsime 把它变成中英文切换键：**按一下切换到另一种输入状态，
大写锁状态与指示灯一动不动**（按下与抬起都被完整拦截，不触达系统）。
`Ctrl/Alt/Shift/Win + CapsLock` 一律放行，长按不重复触发，只拦截这一个键。

纯 Rust 实现，不联网、不装驱动、不注入其他进程；每次切换都写日志，附带诊断工具。

## 特性

- ⌨️ **CapsLock 中英文一键切换** —— 只拦"单独按下的 CapsLock"，其余组合键原样保留
- ⌥ **Alt+C / Alt+V 复制粘贴（macOS 风格）** —— 把 `Alt+C` / `Alt+V` 翻译成 `Ctrl+C` / `Ctrl+V`，
  同时**默认关掉 GUI 应用里的原生 Ctrl+C/V**（只留 Alt 组合）；终端窗口例外，Ctrl+C 终止命令照常。
  没有交换 Ctrl 和 Alt；托盘菜单可一键屏蔽/恢复，映射表也能在配置里改
- 💬 **切换时给个提示** —— 在插入点/输入框旁边弹出 **Apple 系统蓝**的「中 / en」小卡片（白字，亮/暗模式各一档色深），约 0.7 秒淡出；
  拿不到插入点就退到输入区域、再退到鼠标位置；亮/暗模式下自动调整描边与投影，可在托盘菜单关掉
- 🎨 **会看眼色的托盘图标** —— 几何圆环 + 一笔朱砂收锋的「C」，跟随系统亮/暗模式自动换色（右键菜单也跟着变深/浅），暂停时转冷灰
- 🪶 **轻量** —— 单文件 exe 约 350 KB；空闲工作集约 11 MB、私有内存 1.6 MB
- 🍃 **零轮询** —— 钩子回调只投递一次消息；空闲 CPU 实测 15 秒内增长 0 ms
- 🔍 **可排查** —— 每次切换记录方式/状态/是否核对/前台程序；自带 `--selftest` 与 `ime_probe` 探针
- 🧩 **无第三方服务** —— 不联网、无遥测、无管理员权限、不注入其他进程
- 🖥️ **按 DPI 渲染图标** —— 100%–200% 缩放下都是按托盘实际像素尺寸现画，不糊

## 快速开始

### 直接用（推荐）

1. 到 [Releases](https://github.com/Sereinme/capsime/releases) 下载 `capsime.exe`（Windows 10/11 x64）
2. 双击运行，托盘出现「C」图标
3. 在任意程序里按一下 **CapsLock**，中英文来回切换

### 从源码构建

```powershell
git clone https://github.com/Sereinme/capsime.git
cd capsime
cargo build --release        # 产物：target\release\capsime.exe
```

需要 Rust 1.85+（MSVC 或 GNU 工具链均可）。发布构建带 LTO + strip；CI（MSVC）产物约 350 KB，本机 GNU 工具链构建约 472 KB。

## 使用

**托盘菜单**

| 菜单项 | 说明 |
| --- | --- |
| 启用大写键切换 | 临时暂停/恢复拦截；暂停后 CapsLock 恢复原生大写锁功能，图标转冷灰 |
| 开机自动启动 | 写/删注册表 `HKCU\...\CurrentVersion\Run`（只影响当前用户） |
| 切换时显示提示 | 在光标/输入框附近弹出「中 / en」小卡片（默认开） |
| 屏蔽原生 Ctrl+C/V | 勾选后 GUI 应用里 Ctrl+C/V 不再复制粘贴（Alt 组合仍可用，终端保留 Ctrl+C）；设置写进配置、重启后保持 |
| 编辑配置… | 打开 `%LOCALAPPDATA%\capsime\config.ini`（不存在则创建），改卡片底色与位置偏好；保存即生效 |
| 查看日志 | 打开 `%LOCALAPPDATA%\capsime\capsime.log`（超过 512 KB 自动轮转一次） |
| 退出 | 卸载钩子后退出 |

**命令行**

```
capsime                 启动（托盘常驻）
capsime --selftest [秒] 冒烟自检：装钩子、投递一次切换、自动退出（默认 3 秒）
capsime --version       显示版本
capsime --help          显示帮助
```

**想临时用原生大写锁**：托盘菜单暂停，或按 `Shift+CapsLock`（组合键不拦截，
但注意它同时会关闭大写锁，属系统原生行为）。

## 它是怎么工作的

```
CapsLock 按下
   │
   ▼
主线程：WH_KEYBOARD_LL 低级键盘钩子 —— 只做 PostMessage，立刻返回
   ▼
工作线程：注入输入法自己的切换键 Ctrl+Space → 读回状态核对 → 写日志
```

一句话：**注入 `Ctrl+Space` 等价于你自己按切换键**，按键由输入法在目标程序自己的
上下文里处理，所以对任何程序都适用。为什么不直接调用 IMM 接口？
——因为 `ImmGetContext` 对别的进程一律返回 NULL，而默认 IME 窗口读回来的状态
在 TSF 输入法的程序里是**假的**（实测：输入法已经切到中文，它仍坚持报英文）。

细节、实测数据与可复现的探针用法见 **[docs/ime-switching.md](docs/ime-switching.md)**。

## 什么时候别用它

- 你的输入法没有把中英文切换绑在 `Ctrl+Space` 上（先去输入法设置里改回来；capsime 默认注入这个键）
- 少数带内核级反作弊的游戏会屏蔽全局键盘钩子，游戏内无效（不影响系统其他程序）
- UAC 安全桌面（提权弹窗界面）上的按键收不到，属系统机制，不影响已登录桌面
- 你就是想保留原生 CapsLock —— 这个程序的目标正是替掉它（可随时托盘暂停）

## 常见问题

<details>
<summary><b>按 CapsLock 没反应？</b></summary>

1. 看日志 `%LOCALAPPDATA%\capsime\capsime.log`：应出现"已切换到中文/英文（方式：输入法切换键…）"
2. 托盘图标是不是灰的？灰 = 已暂停，右键勾回"启用大写键切换"
3. 跑 `capsime --selftest`：能打印 `OK: 键盘钩子已安装` 说明钩子正常；失败会有明确错误码（常见是安全软件拦截）
4. 跑 `cargo run --example ime_probe -- --notepad`，用"打字结果"直接看这台机器上哪条路径有效
</details>

<details>
<summary><b>日志里的"未核对"是什么意思？</b></summary>

说明这次切换**没有独立的读回值可以核对**——现代程序（TSF 输入法，如记事本）读不回
输入法状态，工具只能如实标注，不代表切换失败。请以屏幕上的输入法状态为准。
</details>

<details>
<summary><b>提示是贴在哪里的？为什么有时候在光标旁边？</b></summary>

按四级阶梯找位置，尽量贴着你正在打字的地方：

1. **Win32 插入点**（`GetGUIThreadInfo`）—— 经典编辑控件都会给；
2. **UIA 插入点** —— 浏览器、Electron、WinUI 不创建 Win32 光标，改问 UI Automation 的
   `TextPattern`（实测与 .NET 的 UIA 读数逐位一致）；
3. **UIA 输入区域** —— 有的程序（Chromium 系、Windows Terminal）只给输入框不给插入点，
   微信这类 Qt 应用更是只给一个整窗；这时退到焦点控件的外框——文本控件取左下，整窗/
   面板取**底边中点**（聊天框、终端、编辑器的输入区几乎都在应用底部）；
4. **鼠标位置** —— 焦点根本不在输入框上时才退回这里。

macOS 的提示能永远贴着输入框，是因为它的文本系统要求每个可编辑视图上报插入点；
Windows 没有这个统一契约。走了哪一级会写进日志（用 `CAPSIME_DEBUG=1` 启动），
想先看看你这台机器上的程序走哪条路：`cargo run --example caret_probe`。
原理与逐程序实测见 [docs/ime-switching.md](docs/ime-switching.md) 的「切换提示」一节。

想改颜色、位置或键位映射：托盘菜单「编辑配置…」→ 改 `config.ini`，保存后下一次切换即生效，
不用重启：

- `light_color` / `dark_color`（RRGGBB）：卡片底色，默认 Apple 系统蓝（亮色 `#0066CC`、暗色 `#0A84FF`）；
- `position`：`auto`（默认，右下方、贴边自动翻转）、`above` / `below`（正上/正下，水平居中于锚点）、
  `left` / `right`（左/右，垂直居中），首选方向放不下时自动翻到对面；
- `alt_as_ctrl`：要翻译成 Ctrl 的 Alt 组合，默认 `c, v`（即 Alt+C/V 复制粘贴）；留空即关闭；
- `ctrl_copy_paste`：原生 Ctrl+C/V 的处理，`gui`（默认，GUI 里吞掉、终端保留）、`keep`（完全保留）、
  `off`（一律吞掉）。终端按窗口类识别：Windows Terminal、conhost、mintty、ConEmu、PuTTY。
</details>

<details>
<summary><b>切换有延迟吗？会不会拖慢打字？</b></summary>

按键注入本身 < 1 ms，按下即生效。单次处理约 110 ms，其中 100 ms 是刻意的等待
（让系统和输入法处理完再确认是否被当成了"切换输入语言"，必要时回滚），
全部发生在工作线程，不阻塞钩子，也不影响打字。
</details>

<details>
<summary><b>怎么卸载？</b></summary>

托盘菜单"退出"，然后删掉 exe 即可。若开过开机自启，先在托盘菜单里取消
（或删除注册表 `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` 下的 `capsime` 项）。
程序不写系统目录、不装驱动，没有其他残留。
</details>

## 性能

本机（Windows 11 x64，release 构建）实测：

| 指标 | 实测值 |
| --- | --- |
| 空闲内存 | 工作集约 10 MB，私有字节 1.6 MB |
| 空闲 CPU | 15 秒采样内增长 0 ms（0.0000% 单核）——事件驱动，无轮询 |
| 线程 / 句柄 | 2 个线程（主线程 + 切换工作线程）/ 约 140 个句柄 |
| 用过提示之后 | 工作集约 17 MB、私有字节 2.9 MB、10 个线程、203 个句柄——UIA 客户端（`uiautomationcore`）的常驻开销，只在真正用到时加载；托盘菜单关掉提示就完全不加载 |
| 单次切换 | 约 120～140 ms（含 100 ms 刻意等待与 14～25 ms 的 UIA 定位；注入本身 < 1 ms） |
| 每次切换 CPU | 约 1 ms（含写日志与 UIA 查询） |
| 连按 100 次后 | 工作集/私有内存/句柄/线程数完全不变（无泄漏） |
| 启动到托盘就绪 | < 1 s |
| release 体积 | 约 350 KB（CI 的 MSVC 构建；本机 GNU 约 472 KB，LTO + strip） |

钩子回调只做"是不是 CapsLock + 有没有修饰键"的判断和一次 `PostMessage`，
普通按键只是两次原子读加一个整数比较，因此全局打字速度不受影响，
也不会触发系统的 `LowLevelHooksTimeout`（默认 300 ms，差四个数量级）。

## 兼容性与已知限制

- 支持 Windows 10 / 11（x64）；ARM64 未测试
- 切换走"注入 `Ctrl+Space`"，微软拼音、搜狗拼音、QQ 拼音等默认都支持；
  若你的输入法把它设成了"无"，切换会失败并在日志里如实记录
- 个别程序的输入法由 TSF 独占，读不回状态，日志会显示"未核对"（正常现象）

## 开发

```powershell
cargo fmt
cargo clippy                # 无警告
cargo test                  # 36 项单元测试
cargo run --example icon_preview        # 托盘图标验收图：尺寸 × 启用/暂停 × 亮/暗
cargo run --example indicator_preview   # 切换提示卡片验收图：字形 × 主题 × 100%/150% DPI
cargo run --example caret_probe         # 输入位置探针：当前程序走哪条定位路径
cargo run --example ime_probe           # 输入法切换排查：环境 + 逐条路径实测
```

```
src/
  main.rs       入口（--version / --help / --selftest 与托盘模式分发、DPI 感知声明）
  app.rs        单实例、双线程消息循环、装配与生命周期、主题广播与提示通知
  hook.rs       WH_KEYBOARD_LL 钩子：只拦截单独按下的 CapsLock
  switcher.rs   中英文切换：读状态 + 注入切换键 + 逐级兜底与核对（含单元测试）
  tray.rs       托盘图标与菜单
  icon.rs       托盘图标：几何圆环 + 朱砂收锋的「C」，亮/暗双主题（含单元测试）
  indicator.rs  切换提示：分层弹窗 + 自绘卡片 + 四级定位阶梯（插入点/输入框/鼠标，含单元测试）
  uia.rs        UI Automation 客户端：手写最小 COM 接口，读浏览器/Electron/WinUI 的插入点
  config.rs     本地配置：卡片配色（config.ini，含单元测试）
  theme.rs      系统亮/暗模式检测
  autostart.rs  开机自启（HKCU Run 项，含单元测试）
  log.rs        极简文件日志（512 KB 轮转）
  util.rs       Win32 小工具（含单元测试）
examples/
  icon_preview.rs       托盘图标验收图生成
  indicator_preview.rs  切换提示卡片验收图生成
  caret_probe.rs        输入位置探针（Win32 / UIA / 鼠标逐级读数）
  ime_probe.rs          输入法切换排查工具（环境检查 / 逐条路径实测 / 真实记事本验证）
assets/icon/       图标设计稿（SVG）、预览图、仓库社交预览卡片与头像图
docs/              设计与实现笔记
```

图标和提示卡片都是纯代码绘制的（无图片资源）：超椭圆/圆角矩形底 + 渐变 + 顶缘高光 +
柔和投影，逐像素 4×4 超采样抗锯齿，按所在显示器 DPI 直接渲染；提示卡片照 Apple 的公开
设计系统（无装饰性渐变、发丝线、系统灰暗色底）用分层窗口（`UpdateLayeredWindow`）拿到
逐像素透明、点击穿透、不抢焦点，文字交给 GDI + 系统字体。读 UI Automation 用的是手写的
最小 COM 接口——它和托盘图标一样，**没有引入任何新依赖**。

## 发布

推一个带注释的 tag 即可，[release.yml](.github/workflows/release.yml) 会自动跑格式检查、
clippy、单元测试、release 构建，打包成 `capsime-v<版本>-x64.exe` 并发布；release notes
由 **tag 说明 + 自上个 tag 以来的提交** 自动生成：

```powershell
git tag -a v0.1.2 -m "一句话摘要（会成为 release notes 的开头）"
git push origin v0.1.2
```

只想验证构建、不发布：Actions → release → Run workflow（手动触发只做测试与构建）。

## 路线图

- [ ] 扩展配置文件：自定义注入的切换键、排除的程序（配色已支持）
- [ ] 长按 CapsLock = 原生大写锁（短按切换中英文）
- [ ] 按住 CapsLock 临时英文（松开回到中文）


## 文档

- [中英文切换是怎么做的（及实测数据，含「切换提示」为什么有时跟光标）](docs/ime-switching.md)

## 许可

MIT，见 [LICENSE](LICENSE)。图标配色与材质参考自
[awesome-design-md](https://github.com/VoltAgent/awesome-design-md) 收录的公开设计系统，
不主张任何第三方品牌视觉身份。