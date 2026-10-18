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

## 切换提示：为什么有时候跟光标，有时候跟输入框

macOS 的输入法切换提示总能贴在输入框旁边，因为它有一套**系统级契约**：任何可编辑文本的
视图都必须实现 `NSTextInputClient`，其中 `firstRectForCharacterRange:` 就是"我的插入点在哪"。
IME 候选窗靠它定位，切换提示自然复用同一信息。

Windows 没有这样的统一契约，等价信息散在三处且都拿不到或不可靠：

| 来源 | 为什么用不上 |
| --- | --- |
| IME 的 `COMPOSITIONFORM`（应用报给 IME 的插入点） | 只有 IME 自己读得到，且只在组字期间有效 |
| `GetCaretPos()` | caret 是**线程私有**的，只能读自己线程的 |
| TSF `ITfContextView::GetTextExt` | 只有文本服务（IME）自己能调 |

capsime 于是走一条四级阶梯，前两级是"插入点"，后两级是兜底：

1. **Win32 插入点** —— `GetGUIThreadInfo`（`idThread = 0` 表示前台线程）跨进程给出
   `hwndCaret` 与 `rcCaret`：
   - `rcCaret` 是 **`hwndCaret` 的客户区坐标**（实测：文本框里把光标放到第 20 个字符，
     x ≈ 223 正是客户区横坐标），所以用 `ClientToScreen` 换算；这个调用跨进程安全，
     而且由 Windows 负责目标窗口的 DPI 映射；
   - 但它只在光标"正在显示"时才给：WinUI 记事本实测忽有忽无，不能当唯一来源。
2. **UIA 插入点** —— UI Automation 的 `TextPattern`（浏览器、Electron、WinUI 都实现了）：
   `TextPattern2.GetCaretRange()` 直接给插入点范围，不行再退回
   `TextPattern.GetSelection()` 取最后一个范围（多行选区时它的末端才是插入点），
   最后 `GetBoundingRectangles()` 拿矩形。实测与 .NET 的 `System.Windows.Automation`
   **逐位一致**：记事本（WinUI）给出 `1157,957,1,52`、Firefox 网页 textarea 给出
   `1228,1912,1,29`；查询耗时 14–17 ms，且跑在切换工作线程上（连接/事务超时收紧到
   250 ms，前台程序卡死也不会拖住钩子线程）。
3. **UIA 输入区域** —— 有的程序给了 `TextPattern` 却**不给矩形**（Chromium/Electron 的
   网页输入框、Windows Terminal），有的干脆只有空壳（微信这类 Qt 应用：焦点是整窗、
   树里没有文本接口）。这时退到焦点控件的 `BoundingRectangle`，取点规则：
   - **文本控件（`Edit` / `Text`）取左下**——光标通常从左侧开始；
   - **其余（整窗/面板/文档）取底边中点**——聊天框、终端、编辑器的输入区几乎都在应用
     底部。早先一律取左下，结果微信里卡片会被甩到窗口左下角，离输入框很远。
4. **鼠标位置** —— 都拿不到（比如焦点根本不在输入框上）才退回光标，永远可用。

前三级共用同一套合理性校验：尺寸在 (0,200] 像素内（插入点）或非退化（外框）、落在外观
可见的前台窗口附近、落在显示器工作区内，任何一条不满足就降到下一级。

用了哪一级会写进诊断日志（`CAPSIME_DEBUG=1` 启动）：

```
DEBUG UIA 输入位置：（1157, 1009）（耗时 14 ms，焦点控件 Document（50030））
DEBUG UIA 输入位置：无插入点，退到输入区域（2229, 1896）（耗时 15 ms，焦点控件 Window（50032））
DEBUG 指示器：锚点来自插入点（1157,1009)，DPI 144，卡片 36px 放在 (1151,1003)
DEBUG 指示器：锚点来自输入区域（UIA）（2229,1896)，DPI 144，卡片 36px 放在 (2223,1890)
```

本机实测清单（`cargo run --example caret_probe` 可复现）：

| 前台程序 | Win32 插入点 | UIA | 提示落在哪 |
| --- | --- | --- | --- |
| 记事本（Win11 / WinUI） | 忽有忽无 | ✅ 插入点（`Document`） | 插入点 |
| Firefox（`zen.exe`）网页 textarea | ✅（与 UIA 同一位置） | ✅ 插入点（`Edit`） | 插入点 |
| ZCode（Electron / Chromium）聊天框 | ❌ | ⚠️ 只给外框 | 输入框左下 |
| Windows Terminal | ❌ | ⚠️ 给了范围但不给矩形 | 窗口底边中点 |
| 微信（Qt） | ❌ | ❌ 只有空壳（`Window`） | 窗口底边中点 |
| 焦点不在输入框上（如阅读网页） | ❌ | ❌ | 鼠标位置 |

### UIA 为什么要手写 COM 接口

windows-sys 只生成 UIA 的 **provider 侧** flat API（`TextRange_*` 那批），没有客户端的
`IUIAutomation` 等 COM 接口；引入官方 `windows` crate 又会带进一整套元 crate。
`src/uia.rs` 因此按 vtable 槽位手写最小接口：**槽位顺序与 IID 逐条取自
Windows SDK 10.0.16299 的 `um/UIAutomationClient.h`**（脚本按 Vtbl 结构体定义顺序提取，
含 3 个 IUnknown 槽位），并与 .NET 的 UIA 读数逐位核对过——同一次查询两边给出的
插入点矩形完全一致。

## 提示卡片本身是怎么画的

不引入 GUI 框架，用**分层窗口 + 自绘**。卡面是 **Apple 系统蓝 + 白色字形**，亮/暗两套
主题各一档（与 macOS 的强调色同一套语言），其余照 Apple 的公开设计系统：**没有装饰性
渐变**——纯色卡面 + 1px 发丝线 + 一道柔和投影。白字要求卡面够深，而"在深色内容上显眼"
又要求卡面够亮，所以按主题各优化一头（相对亮度 Y 与对比度实测）：

| 主题 | 卡面 | Y | 白色字形 | 对浅色内容 | 对深色内容 |
| --- | --- | --- | --- | --- | --- |
| 亮色 | `#0066CC` Action Blue | 0.22 | 3.8:1 | 4.3:1 | 4.3:1 |
| 暗色 | `#0A84FF` macOS 系统蓝 | 0.24 | 3.7:1 | 4.1:1 | 4.6:1 |

被否掉的方案也记在这儿：白/系统灰总有一边看不见；克莱因蓝 `#002FA7`（Y≈0.05）在深色
界面上只有 1.5:1；纯黄（Y≈0.69）在浅色页面上只有 1.3:1，白字还会发虚；朱砂橙够显眼，
但"不够高级"，最终换回 Apple 自己的蓝。亮/暗主题同时体现在发丝线（浅色黑线/深色白线）、
上缘高光与投影强度上。

### 配置：配色与位置

配置写在 `%LOCALAPPDATA%\capsime\config.ini` 里（托盘菜单「编辑配置…」会创建模板并用默认
编辑器打开）：

```ini
# 卡片底色，RRGGBB（可带 #）
light_color = 0066CC
dark_color = 0A84FF

# 相对锚点（插入点/鼠标）的位置：auto / above / below / left / right
position = auto
```

`position` 的含义：

| 值 | 摆在哪 | 放不下时 |
| --- | --- | --- |
| `auto`（默认） | 右下方 | 翻到左侧 / 上方 |
| `above` / `below` | 正上 / 正下，水平居中于锚点 | 翻到对面 |
| `left` / `right` | 左 / 右，垂直居中于锚点 | 翻到对面 |

改完保存即可——卡片每次显示前会看一眼文件的修改时间，变了就重读并写一行日志
（`配置：亮色 #0066CC、暗色 #0A84FF、位置 auto`），不需要重启；格式错误的行会被忽略并记
`WARN`，缺项用默认值。

- 卡片 **24 逻辑像素**（150% 缩放下 36px、125% 下 30px），与锚点的间距是**看得见的
  6 物理像素**——注意窗口比卡片大一圈**阴影留白**（边长的 34%，且随 DPI 变大），所以
  摆放时要把留白从偏移里扣掉；否则"逻辑偏移固定"了，看得见的间距仍是 150% 屏更大
  （18px vs 16px），两块屏一对比 2K 就显得远；
- 形状是**超椭圆**（Apple 式连续
  圆角方砖，`|u|⁴ + |v|⁴ = 1`），按锚点所在显示器的 DPI 现画；
- 窗口：`WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE`，
  用 `SetWindowPos(..., SWP_NOACTIVATE | SWP_SHOWWINDOW)` 显示——**逐像素透明、点击穿透、
  不抢焦点**（抢焦点会让 IME 候选窗弹错位置）；
- 位图：自己按像素画（超椭圆 + 1px 发丝线 + 暗色上缘微高光 + 柔和投影（照 Apple 产品阴影的比例向右下偏移，smoothstep 衰减且只在下半区出现，避免四周围一圈灰边），
  4×4 超采样），文字用 GDI + 系统字体画成掩膜、按墨迹包围盒做光学居中。两个字形的
  **笔画粗细用"2×面积/周长"量过并配对**：中 = YaHei UI Bold（3.71px）、en = Segoe UI
  Bold（3.64px）——Semibold 是 2.67px，配哪边都不像同一字重（这就是"中英文看起来
  字重不一样"的成因）；
- 交给 `UpdateLayeredWindow`（`ULW_ALPHA`，32bpp **预乘** alpha）显示；
- 动画由 `SetTimer` 驱动：3 帧淡入（120ms）→ 12 帧停留（480ms）→ 4 帧淡出（160ms），
  没有轮询。

体积代价：**0 个新依赖**（UIA 那部分也是手写 COM，没引入 `windows` crate）、
约 1200 行代码（含 UIA 与测试）；作为对比，引入 egui/Slint 这类框架会把
335 KB 的单文件变成数 MB，还要额外跟框架搏斗才能做到不激活与逐像素透明。

运行代价：第一次真正查 UIA 之后，进程会常驻多出 8 个线程、工作集约 +7 MB
（`uiautomationcore` 的连接与回调线程；实测把 COM 套间在每次查询后还回去也回收不掉），
之后不再增长，空闲时也不动；托盘菜单关掉提示就完全不加载 UIA。另外，查询会让
浏览器/Electron 常驻开启它们的无障碍引擎——这是任何 UIA 客户端都会有的副作用
（也正是"插入点"这份信息的来源），同样可以用托盘开关完全避免。