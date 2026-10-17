# 中英文切换是怎么做的（以及为什么只能这么做）

本文记录 capsime 的核心结论与实测数据。所有结论都来自在本机（Windows 11 + 微软拼音）
上的实测，**判据是前台程序里的打字结果**（输入 `nihao` 得到「你好」还是 `nihao`），
而不是任何 API 的返回值——这一点很关键，下面会看到为什么。

## 结论

**唯一可靠的切换动作是"注入输入法自己的中英文切换键（Ctrl+Space）"。**

它等价于用户自己按切换键：按键由输入法在**目标程序自己的上下文**里处理，
所以对任何程序都适用（经典 Win32、Qt、Electron、WinUI3 都实测有效），跨进程也一样。

## 反面证据：两条"看起来能用"的路

### 1. `ImmGetContext` 对别的进程一律返回 NULL

首选方案本来是对前台窗口调 IMM：`ImmGetContext` + `ImmSetConversionStatus`。
实测在 Windows 10/11 上，这个调用对**其他进程**的窗口一律返回 NULL：

```
[Notepad.exe] class=Notepad       imm=(ctx=NULL)
[GameViewer.exe] class=Qt5...      imm=(ctx=NULL)
[ZCode.exe] class=Chrome_WidgetWin_1 imm=(ctx=NULL)
[explorer.exe] class=CabinetWClass imm=(ctx=NULL)
[SystemSettings.exe] class=Windows.UI.Core.CoreWindow imm=(ctx=NULL)
```

也就是说，只要前台不是自己的窗口，这条路根本走不到——只能作为"本进程窗口"的补充。

### 2. 默认 IME 窗口读回来的**不是真实状态**

退而求其次，很多人会去读前台线程的默认 IME 窗口：

```c
HWND ime = ImmGetDefaultIMEWnd(hwnd);
SendMessage(ime, WM_IME_CONTROL, IMC_GETCONVERSIONMODE, 0);
```

它在经典窗口上确实跟着状态走（可用作参考），但在 TSF 输入法的程序上**完全失真**。
在记事本里实测（判据是打字结果）：

| 步骤 | 记事本里输入 `nihao` + 空格 | 读回值 |
| --- | --- | --- |
| 基线 | `nihao`（英文） | `conv=0x0001 open=0`（EN） |
| 注入一次 `Ctrl+Space` 后 | **`你好`（中文）** | `conv=0x0001 open=0`（EN，**没变**） |

输入法明明已经切到中文，读回值却坚持说英文，而且注入切换键后它纹丝不动。
更糟的是往这个窗口发 `IMC_SETCONVERSIONMODE`：**它只改自己的读回值**，
前台程序的实际输入状态毫无变化——这正是"日志说已切换、实际没切"的根源。

## 由此得到的实现

```
CapsLock 按下
   │
   ▼
主线程：WH_KEYBOARD_LL 低级键盘钩子
   │  · 只在"无修饰键 + 物理按键 + 非长按重复"时拦截
   │  · 吞掉按下/抬起 → 大写锁状态与指示灯不变
   │  · 只做 PostMessage(WM_SWITCH)，立刻返回（避免钩子超时被系统移除）
   ▼
工作线程：对前台窗口切换中英文
   1. IMM 上下文：拿到就精确设置（本进程窗口才有；读-写-复核）
   2. 注入输入法切换键 Ctrl+Space（主路径，一次按键只注入一次）
      · 不做无依据的重试：状态读不回时重试会把状态翻回去 = 按了没反应
      · 若系统把这次按键当成"切换输入语言"：切成中文/英文布局算成功；
        切成无关语言（如日文）立刻切回，转用 3
   3. 键盘布局切换（WM_INPUTLANGCHANGEREQUEST，≈ Win+Space）：最后的兜底
   ▼
写日志：方式、切换后状态（读得回时）、是否核对过、前台程序名
```

"中文态"的判定：IME 处于打开状态 **且** 转换模式带 `IME_CMODE_NATIVE` 位。

跨进程的同步消息（读默认 IME 窗口）一律用 `SendMessageTimeoutW` + `SMTO_ABORTIFHUNG`
并带 250ms 超时：前台程序卡死时立刻放弃，绝不拖住工作线程，更不会卡住钩子。

## 自己动手验证

仓库自带探针，可复现上面每一条结论：

```powershell
cargo run --example ime_probe               # 环境 + 逐条路径实测（进程内黄金判据）
cargo run --example ime_probe -- --notepad  # 真实记事本里验证「打字结果」
cargo run --example ime_probe -- --e2e      # 对着另一个真实进程验证切换链路
```

`--notepad` 是最可信的一档：它新开一个记事本，在**空文档**里输入 `nihao` + 空格，
再看标题变成「你好」还是 `nihao`（安全阀：只在"未命名且无内容"的窗口里打字，
且注入前会再确认前台窗口是它自己）。

本机实测有效的清单（`--notepad` 可复现）：

| 方法 | 能否真正改变前台程序的输入状态 |
| --- | --- |
| IMM `ImmSetConversionStatus`（本进程窗口） | ✅ |
| 注入 `Ctrl+Space` | ✅ |
| 默认 IME 窗口 `IMC_SETCONVERSIONMODE` | ❌ 只改自己的读回值 |
| `Shift` / 单独 `Ctrl` / `IME_ON`·`IME_OFF` 虚拟键 / `ImmSimulateHotKey` / `VK_KANA` | ❌ |

> 换输入法/换机器时请以 `--notepad` 的结果为准：不同输入法的中英文切换键可能不同
> （capsime 默认注入 `Ctrl+Space`，微软拼音默认如此）。