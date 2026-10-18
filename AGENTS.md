# AGENTS.md

给在本仓库工作的编码代理的说明。面向用户的介绍、截图与 FAQ 见 [README.md](README.md)。

## 项目

capsime —— Windows 10/11 上用 CapsLock 切换中英文输入法的托盘小工具。纯 Rust、单文件 exe
（当前 ≈464 KB）；不联网、不注入其他进程、不需要管理员权限。Win32 走 `windows-sys`，
托盘用 `tray-icon`/`muda`，图标与切换提示卡片全部自绘。

## 常用命令

```powershell
cargo fmt
cargo clippy --all-targets -- -D warnings
cargo test                             # 36 项单元测试
cargo build --release                  # 产物 target/release/capsime.exe

cargo run --example icon_preview       # 托盘图标验收图
cargo run --example indicator_preview  # 提示卡片验收图（字形 × 主题 × 尺寸阶梯）
cargo run --example caret_probe        # 输入位置探针（Win32 / UIA 输入框 / 鼠标，逐级）
cargo run --example ime_probe          # 输入法切换排查（--notepad 最可信）
```

验收图输出到 `target/`（已被忽略），不要提交。改外观后先看验收图再提交。

## 发布（自动化）

推**带注释的 tag** 即自动发布，不需要手工构建与上传：

```powershell
git tag -a vX.Y.Z -m "一句话摘要（会成为 release notes 的开头）"
git push origin vX.Y.Z
```

`.github/workflows/release.yml` 依次执行：

1. 检出源码（`fetch-depth: 0`，生成更新日志需要完整历史）
2. `cargo fmt --check` → `cargo clippy --all-targets -- -D warnings` → `cargo test`
3. `cargo build --release`
4. 打包为 `capsime-v<版本>-x64.exe`，计算大小与 SHA256
5. 生成 release notes：**tag 说明 + 自上个 tag 以来的提交（带链接）+ 下载校验块**
6. `gh release create --verify-tag`；标题为「tag · tag 说明里冒号后的摘要」

发布前检查：

- `Cargo.toml` 的 `version` 与 tag 一致（tag `v0.1.2` ↔ `version = "0.1.2"`）
- README / docs 里的实测值同步（exe 体积、测试数、内存等）
- 先 `git push origin HEAD` 再推 tag；tag 必须是正式版本号——`v*` 一律触发发布
- 首次发布或想验证流水线：Actions → release → Run workflow（`workflow_dispatch`
  只跑测试与构建，不发布会），或本地跑上面「常用命令」里的三步

## 约定与雷区

- **0 新依赖是倾向**：读 UIA 用的是手写的最小 COM 接口（`src/uia.rs`，槽位/IID 抄自
  SDK 头文件并在模块文档里留了核对表），不要随手换成 `windows` crate
- 全局键盘钩子回调里只允许 `PostMessage`；任何可能阻塞的调用（UIA、文件 IO）都放
  工作线程（`app::do_switch`）
- 几条单元测试是**有意的约束**，改配色/尺寸时别绕过：
  - `card_face_is_conspicuous_in_both_themes`：两套主题的卡面都要白字可读（≥3:1）、
    对浅色页与深色内容都 ≥2.8:1、彩度 ≥100
  - `visible_gap_is_constant_across_dpi`：卡片与锚点"看得见的间距"在 96–192 DPI 下恒为 6px
  - `squircle_is_rounder_than_a_square`：卡片形状是超椭圆，不是圆角矩形
- 摆放提示卡片时要把**阴影留白**从间距里扣掉（`place(anchor, side, OFFSET_PX - margin, …)`），
  否则 150% 屏上会比 125% 屏看得更远
- 数据目录：`%LOCALAPPDATA%\capsime\`（`capsime.log` 与 `config.ini`）；配色配置
  改完保存即生效（按文件时间戳自动重载），不需要重启
- 中文提交信息：本机 PowerShell 5.1 可能按 GBK 解码外部命令输出，写脚本处理 git 输出时
  记得把 `$OutputEncoding` / `[Console]::OutputEncoding` 设成 UTF-8