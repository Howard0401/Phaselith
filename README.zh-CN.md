# Phaselith

**[English](README.md)** | **[繁體中文](README.zh-TW.md)** | **简体中文**

Phaselith 是一个实时感知音频修复与呈现引擎，由 Phaselith 引擎驱动。

Phaselith 让受损、有损压缩、空间塌缩或刺耳的播放音频听起来更聚焦、更清晰，并在听者面前产生更真实的物理定位感。

本仓库包含 Phaselith 核心、浏览器运行环境、Windows APO 运行环境，以及将 DSP 引擎转化为实际产品所需的控制层代码。

## 项目目标

Phaselith 旨在回答一个简单的问题：

软件能在多大程度上将日常播放推向专业系统的呈现品质，而不沦为噱头、固定 EQ 曲线，或单一空间效果？

项目聚焦五个目标：

1. 减少遮蔽、雾感、刺耳感和类 codec 的粗糙感。
2. 强化中心结像和前方音源定位。
3. 保留音乐的密度和冲击力，而非将一切压平成「清晰」。
4. 在普通播放环境中运作，包括浏览器、笔电喇叭、耳机和系统级桌面音频。
5. 保持可解释、可测试、可出货的工程系统。

## Phaselith 不只是 EQ

EQ 套用固定或半固定的音调曲线。

Phaselith 的做法是：

1. 估测当前存在何种损伤或塌缩。
2. 将信号分解为谐波、空气感、瞬态、空间和相位相关结构。
3. 构建可能修复缺失或被抑制的感知结构的候选残差。
4. 通过重投影步骤自我验证这些残差，而非盲目加回。
5. 经由安全层混合结果，保护峰值、低频稳定性和长期可听性。

这就是为什么 Phaselith 能在不像传统 EQ 的情况下，听起来像是减少了混浊、恢复了前方聚焦，或改善了结像锁定。

## 核心算法

Phaselith 引擎组织为 M0-M7：

1. `M0 Orchestrator` — 缓冲主机回调、管理帧与跳步时序、提供对齐的分析窗口。
2. `M1 Damage Posterior` — 估测截止频率、削波、限幅、立体声塌缩和置信度。
3. `M2 Tri-Lattice` — 产生后续模块读取的分析晶格。
4. `M3 Factorizer` — 分离谐波、空气感、瞬态和空间场。
5. `M4 Inverse Residual Solver` — 在频率和时间域中产生候选修复。
6. `M5 Self-Reprojection Validator` — 拒绝或缩减未通过退化一致性测试的修复。
7. `M6 Perceptual Safety Mixer` — 混合已验证残差，搭配响度补偿、特性塑形和氛围保留机制。
8. `M7 Governor` — 发布遥测和运行状态供控制与调试使用。

详细算法说明见 [docs/03-CORE-ALGORITHM.md](docs/03-CORE-ALGORITHM.md)。

## 运行环境

### 浏览器运行环境

- 路径：`chrome-ext -> AudioWorklet -> wasm-bridge -> dsp-core`
- 适用：即时听感验证、浏览器媒体播放、快速出货
- 目前状态：本仓库中最成熟的立体声听感运行环境

### Windows 桌面运行环境

- 路径：`Tauri control panel -> mmap IPC -> APO DLL -> dsp-core`
- 适用：Windows 系统级播放
- 目前状态：可用且具战略重要性，但仍为过渡性立体声运行环境，尚非最终旗舰架构

### 未来目标平台

- macOS：基于 Core Audio 的运行环境
- Linux：基于 PipeWire 的运行环境

路线图与限制见 [docs/08-ROADMAP-AND-LIMITATIONS.md](docs/08-ROADMAP-AND-LIMITATIONS.md)。

## 仓库结构

- `chrome-ext/` — 浏览器扩展运行环境和 AudioWorklet 主机。
- `crates/dsp-core/` — Phaselith 核心。
- `crates/wasm-bridge/` — 浏览器运行环境使用的 WASM 桥接。
- `crates/apo-dll/` — Windows APO 运行环境。
- `crates/tauri-app/` — 桌面控制面板和 APO 管理 UI。
- `docs/` — 编号式架构、算法、运行环境、验证和许可文档。

## 状态

> **早期预览** — Phaselith 正在积极开发中。Phaselith 核心已稳定且产出强劲的听感结果，但 API、配置选项和运行环境架构可能在未通知的情况下变更。

目前浏览器运行环境是本仓库中最干净的听感参考。

Windows APO 路径已具价值，但仍记录为过渡性运行环境，因为其立体声执行模型尚未达到最终的立体声原生设计。

## 许可

本仓库采用 GNU Affero General Public License v3.0 或更新版本许可。

- 完整许可文本见 [LICENSE](LICENSE)。
- 商业许可选项见 [COMMERCIAL-LICENSE.md](COMMERCIAL-LICENSE.md)。
- 贡献条款见 [CONTRIBUTING.md](CONTRIBUTING.md)。
- 防御性公开技术披露见 [docs/10-DEFENSIVE-PUBLICATION.md](docs/10-DEFENSIVE-PUBLICATION.md)。

商业许可适用于希望在无 AGPL 义务下使用、嵌入或重新发布 Phaselith 的团队。
