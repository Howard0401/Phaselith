# Phaselith

**[English](README.md)** | **繁體中文** | **[简体中文](README.zh-CN.md)**

Phaselith 是一個即時感知音訊修復與呈現引擎，由 Phaselith 引擎驅動。

Phaselith 讓受損、有損壓縮、空間塌縮或刺耳的播放音訊聽起來更聚焦、更清晰，並在聽者面前產生更真實的物理定位感。

本倉庫包含 Phaselith 核心、瀏覽器執行環境、Windows APO 執行環境，以及將 DSP 引擎轉化為實際產品所需的控制層程式碼。

## 專案目標

Phaselith 旨在回答一個簡單的問題：

軟體能在多大程度上將日常播放推向專業系統的呈現品質，而不淪為噱頭、固定 EQ 曲線，或單一空間效果？

專案聚焦五個目標：

1. 減少遮蔽、霧感、刺耳感和類 codec 的粗糙感。
2. 強化中心結像和前方音源定位。
3. 保留音樂的密度和衝擊力，而非將一切壓平成「清晰」。
4. 在普通播放環境中運作，包括瀏覽器、筆電喇叭、耳機和系統級桌面音訊。
5. 保持可解釋、可測試、可出貨的工程系統。

## Phaselith 不只是 EQ

EQ 套用固定或半固定的音調曲線。

Phaselith 的做法是：

1. 估測當前存在何種損傷或塌縮。
2. 將訊號分解為諧波、空氣感、瞬態、空間和相位相關結構。
3. 建構可能修復缺失或被抑制的感知結構的候選殘差。
4. 透過重投影步驟自我驗證這些殘差，而非盲目加回。
5. 經由安全層混合結果，保護峰值、低頻穩定性和長期可聽性。

這就是為什麼 Phaselith 能在不像傳統 EQ 的情況下，聽起來像是減少了混濁、恢復了前方聚焦，或改善了結像鎖定。

## 核心演算法

Phaselith 引擎組織為 M0-M7：

1. `M0 Orchestrator` — 緩衝主機回呼、管理幀與跳步時序、提供對齊的分析視窗。
2. `M1 Damage Posterior` — 估測截止頻率、削波、限幅、立體聲塌縮和信賴度。
3. `M2 Tri-Lattice` — 產生後續模組讀取的分析晶格。
4. `M3 Factorizer` — 分離諧波、空氣感、瞬態和空間場。
5. `M4 Inverse Residual Solver` — 在頻率和時間域中產生候選修復。
6. `M5 Self-Reprojection Validator` — 拒絕或縮減未通過退化一致性測試的修復。
7. `M6 Perceptual Safety Mixer` — 混合已驗證殘差，搭配響度補償、特性塑形和氛圍保留機制。
8. `M7 Governor` — 發布遙測和執行狀態供控制與除錯使用。

詳細演算法說明見 [docs/03-CORE-ALGORITHM.md](docs/03-CORE-ALGORITHM.md)。

## 執行環境

### 瀏覽器執行環境

- 路徑：`chrome-ext -> AudioWorklet -> wasm-bridge -> dsp-core`
- 適用：即時聽感驗證、瀏覽器媒體播放、快速出貨
- 目前狀態：本倉庫中最成熟的立體聲聽感執行環境

### Windows 桌面執行環境

- 路徑：`Tauri control panel -> mmap IPC -> APO DLL -> dsp-core`
- 適用：Windows 系統級播放
- 目前狀態：可用且具戰略重要性，但仍為過渡性立體聲執行環境，尚非最終旗艦架構

### 未來目標平台

- macOS：基於 Core Audio 的執行環境
- Linux：基於 PipeWire 的執行環境

路線圖與限制見 [docs/08-ROADMAP-AND-LIMITATIONS.md](docs/08-ROADMAP-AND-LIMITATIONS.md)。

## 倉庫結構

- `chrome-ext/` — 瀏覽器擴充功能執行環境和 AudioWorklet 主機。
- `crates/dsp-core/` — Phaselith 核心。
- `crates/wasm-bridge/` — 瀏覽器執行環境使用的 WASM 橋接。
- `crates/apo-dll/` — Windows APO 執行環境。
- `crates/tauri-app/` — 桌面控制面板和 APO 管理 UI。
- `docs/` — 編號式架構、演算法、執行環境、驗證和授權文件。

## 狀態

> **早期預覽** — Phaselith 正在積極開發中。Phaselith 核心已穩定且產出強勁的聽感結果，但 API、設定選項和執行環境架構可能在未通知的情況下變更。

目前瀏覽器執行環境是本倉庫中最乾淨的聽感參考。

Windows APO 路徑已具價值，但仍記錄為過渡性執行環境，因為其立體聲執行模型尚未達到最終的立體聲原生設計。

## 授權

本倉庫採用 GNU Affero General Public License v3.0 或更新版本授權。

- 完整授權文字見 [LICENSE](LICENSE)。
- 商業授權選項見 [COMMERCIAL-LICENSE.md](COMMERCIAL-LICENSE.md)。
- 貢獻條款見 [CONTRIBUTING.md](CONTRIBUTING.md)。
- 防禦性公開技術揭露見 [docs/10-DEFENSIVE-PUBLICATION.md](docs/10-DEFENSIVE-PUBLICATION.md)。

商業授權適用於希望在無 AGPL 義務下使用、嵌入或重新發布 Phaselith 的團隊。
