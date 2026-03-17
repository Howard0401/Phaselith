# Phaselith FFT Full Implementation Plan

Version: Draft v1
Target branch: `FFT`
Purpose: 提供一份可直接交給 Claude 實作的完整 FFT / STFT / ISTFT / OLA 重構計畫。
Scope: 只規劃，不直接要求一次做完所有重構；採取「先保住現有聲音，再逐步替換核心」策略。

Related documents:
- `23-ARCHITECTURE-REPLAN-REVIEW.md`

---

## 1. 核心目標

這份計畫的真正目標不是把數學變漂亮，而是：

`把目前已經存在的前方結像、端正中心、高級感，從半過渡式重建路徑，升級成真正可維護、可擴充、可驗證的 FFT/OLA 架構。`

成功標準不是「做出 ISTFT」而已。
成功標準是：

1. 不破壞目前的核心魔法
2. 讓頻域路徑有真正的 frame/hop/OLA 語意
3. 讓後續 style、stereo、APO、桌面版有正確基礎
4. 讓未來可以誠實地稱為完整重建路徑

---

## 2. 非目標

這次 FFT 分支不追求以下事情：

1. 不同時重構 APO 終局 stereo-native runtime
2. 不同時做 GPU backend
3. 不同時做 DSD / HQPlayer 式高倍率升頻
4. 不同時大改 style preset 哲學
5. 不把 browser 立即升成全新 host 架構

這份計畫只聚焦：

- `M0/M2/M5` 的 frame-aware FFT 基礎
- 與其直接相依的 context / buffer / config 契約
- 讓 legacy additive 路徑可以被安全地替換

---

## 3. 現況判讀

目前 codebase 的狀態：

### 3.1 已有的優勢

- Browser 路徑已經能產生強烈的前方結像與高級感
- `CrossChannelContext` 與 `time_residual` 域分離已經建立
- `M5` 已脫離最早的 `k % out_len` placeholder
- `M6` 的 loudness compensation 與 headroom-aware makeup 已成形
- mono/stereo APO format negotiation 已收斂到安全範圍

### 3.2 還不是完整重建的地方

- `M2` 仍是「拿目前 block zero-pad」分析，沒有正式 frame/hop 語意
- `M2/stft.rs` 仍是臨時 buffer + planner 模式，不是正式 zero-alloc engine
- `M5` 雖然已可合成內容，但還不是 hop-aligned ISTFT + OLA
- `ValidatedResidual.data` 仍是過渡性 time-domain 容器，不是標準 OLA 輸出介面

### 3.3 風險結論

現在最危險的做法是：

`直接在現有 M2/M5 上硬塞單幀 IFFT，然後假裝那就是完整 FFT 版。`

這條路最容易讓你：
- 技術上變得更複雜
- 聲音卻變差
- 之後還是要重做一次

---

## 4. 總體策略

整體採用兩層策略：

### 4.1 結構策略

先做 `B0`，再做 `B1`。

- `B0` = 建立正確架構與契約，但不急著改聲音主路徑
- `B1` = 在已有 frame/hop/OLA 結構上，導入真正 FFT reconstruction

### 4.2 聲音策略

永遠保留一條 legacy 可比較路徑。

也就是：

- 不要一開始就刪掉現有 additive path
- 要能在同一版 code 中切換：
  - `LegacyAdditive`
  - `FftOlaPilot`
  - `FftOlaFull`

這樣 Claude 每做一步，你都可以直接 AB 聽。

---

## 5. 新的架構目標

完成後的關鍵結構應該長這樣：

```text
Host block (AudioWorklet / APO callback)
    ↓
M0 FrameClock + FrameAccumulator
    ↓
Windowed, hop-aligned analysis frames
    ↓
M2 Tri-Lattice Analysis (zero-alloc STFT engine)
    ↓
M3 Structured Fields
    ↓
M4 Residual Solver
    ↓
M5 Validator
    ↓
M5 Synthesizer (ISTFT + OLA)
    ↓
M6 Safety Mixer + Character Layer
```

重點是：

`M5 必須拆成 Validation 與 Synthesis 兩個責任，不再同時又驗證又隨手把 freq 殘差塞回 time-domain。`

---

## 6. 必須保住的核心聽感

這些是本計畫的硬性 guardrails：

1. singer 在前面
2. center image 很正
3. 耳機不像兩邊各自發聲
4. 沒毛邊
5. 有高級感

任何 phase 只要讓上述特徵明顯退化：

`就算程式更漂亮，也視為失敗。`

---

## 7. Phase 0：分支與基準管理

### 7.1 目標

把目前好聽版本定成 reference baseline，避免 FFT 重構把聲音弄壞後無法回頭。

### 7.2 Claude 要做的事

1. 先確認目前 `FFT` 分支工作樹乾淨
2. 建一個 baseline commit
3. 若需要，再標一個 tag 或在文件中記錄 baseline commit hash

### 7.3 建議 commit 範圍

- tab follow 穩定版本
- M6 loudness compensation v2
- M5 additive synthesis 版本
- APO mono/stereo negotiation 修正

### 7.4 驗收

- baseline 可以穩定 build/test
- baseline 可以當 AB 參考聲音

---

## 8. Phase B0-A：正式定義 frame / hop / runtime contract

### 8.1 目標

把目前隱含存在於 `QualityMode` 裡的 FFT size / hop size，升格成真正的 runtime 契約。

### 8.2 主要改動檔案

- `crates/dsp-core/src/config.rs`
- `crates/dsp-core/src/module_trait.rs`
- 視需要新增：
  - `crates/dsp-core/src/frame.rs`
  - 或 `crates/dsp-core/src/runtime/frame_clock.rs`

### 8.3 要新增的概念

- `host_block_size`
- `core_fft_size`
- `micro_fft_size`
- `air_fft_size`
- `hop_size`
- `analysis_frame_ready`
- `analysis_frame_index`

### 8.4 具體要求

1. 不再默認「當前 callback block = 分析 frame」
2. `QualityMode::hop_size()` 成為正式 runtime contract
3. `ProcessContext` 能區分：
   - host callback index
   - analysis frame index

### 8.5 驗收

- 不改變目前聲音
- 單元測試可驗證 frame clock 推進語意

---

## 9. Phase B0-B：M0 升級成 FrameAccumulator / FrameClock

### 9.1 目標

讓 M0 負責真正的 frame accumulation，而不是只有 ring buffer / dry copy。

### 9.2 主要改動檔案

- `crates/dsp-core/src/modules/m0_orchestrator/*`
- `crates/dsp-core/src/module_trait.rs`
- 視需要新增：
  - `analysis_frame_buffer`
  - `overlap history`

### 9.3 要做的事情

1. host block 進來時先寫入 accumulator
2. 每當累積到一個新 hop，產生對齊好的 analysis frame
3. 提供 M2 可以讀的：
   - micro frame
   - core frame
   - air frame

### 9.4 重要限制

- 這一步先不要改 M5 輸出聲音路徑
- 只建立結構與資料來源

### 9.5 驗收

- 在不同 block size 下都能穩定推進 frame
- 不新增 allocation in hot path

---

## 10. Phase B0-C：建立 zero-alloc STFT engine

### 10.1 目標

把目前 `stft.rs` 的分析流程改成真正的可重用、零配置 hot path。

### 10.2 主要改動檔案

- `crates/dsp-core/src/modules/m2_lattice/stft.rs`
- `crates/dsp-core/src/modules/m2_lattice/mod.rs`

### 10.3 要新增的元件

- `WindowBank`
- `StftScratch`
- `FftHandles` 或 planner cache

### 10.4 新 API 建議

```rust
pub fn analyze_into(
    frame: &[f32],
    lattice: &mut Lattice,
    window: &[f32],
    scratch: &mut [Complex<f32>],
    fft: &dyn Fft<f32>,
)
```

### 10.5 額外要求

- 不在 `process()` 裡建 `Vec`
- 不在每次分析時重建 `FftPlanner`
- analysis window 在 `init()` 預先建立

### 10.6 驗收

- 與現有 `analyze_lattice()` 頻譜結果一致或非常接近
- 全部現有 M2 測試繼續通過

---

## 11. Phase B0-D：先把 M5 拆責任，不急著換聲音

### 11.1 目標

讓 `SelfReprojectionValidator` 不再同時扮演：
- validator
- freq→time converter

### 11.2 主要改動檔案

- `crates/dsp-core/src/modules/m5_reprojection/mod.rs`
- 視需要拆成：
  - `validator.rs`
  - `synthesizer.rs`

### 11.3 新責任分層

#### Validator
- combine residuals
- reprojection degradation
- error computation
- acceptance mask
- constraints

#### Synthesizer
- 從 validated freq residual 生成 time-domain residual
- 支援不同 synthesis mode

### 11.4 建議新增 enum

```rust
pub enum SynthesisMode {
    LegacyAdditive,
    FftOlaPilot,
    FftOlaFull,
}
```

### 11.5 驗收

- 預設仍走 `LegacyAdditive`
- 不改現在聲音
- code 結構更清楚

---

## 12. Phase B0-E：定義 OLA buffer 與 synthesis contract

### 12.1 目標

在真正做 ISTFT 前，先把 synthesis 的資料流與狀態定義清楚。

### 12.2 需要的新元件

- `SynthesisWindow`
- `OverlapAddBuffer`
- `SynthesisScratch`

### 12.3 建議結構

```rust
pub struct OverlapAddBuffer {
    accum: Vec<f32>,
    write_pos: usize,
    hop_size: usize,
    frame_size: usize,
}
```

### 12.4 關鍵規則

- OLA buffer 不直接等於 host block buffer
- OLA read/write 要以 hop size 為節奏
- Browser callback block 只是消費 OLA 已準備好的輸出片段

### 12.5 驗收

- 有明確 API
- 暫時不用正式掛到聲音主路徑

---

## 13. Phase B1-A：Core-lattice pilot FFT reconstruction

### 13.1 目標

先只讓 `core lattice` 走真正 FFT reconstruction，作為 pilot。

### 13.2 為什麼只做 core

因為一次讓 `micro + core + air` 都進 ISTFT 風險太高。
先把主幹打通，才知道聲音有沒有走對。

### 13.3 主要改動檔案

- `crates/dsp-core/src/modules/m5_reprojection/*`
- `crates/dsp-core/src/modules/m2_lattice/stft.rs`
- `crates/dsp-core/src/module_trait.rs`

### 13.4 Pilot 內容

1. 取 validated freq residual
2. 用 `ctx.lattice.core.phase` 建 complex spectrum
3. 做 IFFT
4. 套 synthesis window
5. 丟入 OLA buffer

### 13.5 重要限制

- 先只處理 harmonic / air / phase 路徑
- `time_candidate` 仍維持現有直通
- side path 先不要在這一步加大 aggressiveness

### 13.6 驗收

- `LegacyAdditive` 與 `FftOlaPilot` 可 runtime 切換
- 某些 reference 歌上可直接 A/B
- 不出現 glitch / NaN / 爆音

---

## 14. Phase B1-B：真正的 hop-aligned ISTFT + OLA

### 14.1 目標

將 pilot 擴展成完整、合法的 hop-aligned reconstruction path。

### 14.2 要完成的事情

1. 分析 frame 真正按 hop 推進
2. synthesis frame 用對應 window 合成
3. 透過 OLA 做跨幀累加
4. host callback 從 OLA buffer 取出對齊的輸出

### 14.3 成功條件

- 不再需要 legacy additive 才能產生主力內容
- `ValidatedResidual.data` 的角色更像真正重建輸出

### 14.4 風險

這是最高風險 phase。
任何一個地方錯位都會直接影響：
- center image
- phase coherence
- 空氣感
- punch

### 14.5 驗收

- 無明顯聲音退化
- 與 legacy 相比至少在部分 reference material 上更自然
- 能穩定跑完整測試

---

## 15. Phase B1-C：style / stereo / impact 重新校正

### 15.1 目標

當 FFT/OLA 上線後，重新校正現有 tuning，使其保住原本的 sonic identity。

### 15.2 需要重聽的重點

- 前方結像是否還在
- 低頻 impact 是否變少
- stage 是否變散
- smoothness 是否過頭
- `Grand` 是否仍有招牌的華麗感

### 15.3 可能要調整的元件

- `m4_solver/harmonic_ext`
- `m4_solver/phase_relax`
- `m4_solver/side_recovery`
- `m6_mixer/mod.rs`
- `StyleConfig` preset seed

### 15.4 驗收

- FFT 版不只是「正確」，而是「更完整但仍然像 Phaselith」

---

## 16. APO 路徑在 FFT 分支的定位

這一輪 FFT 分支不要同時決定 APO 終局。

### 可以做

- 讓 APO route 不被新 FFT 結構卡死
- 保持 interface 可未來接上 frame/hop runtime

### 不要做

- 不要在 FFT 分支同時重做 APO stereo-native
- 不要在這一分支處理 APO 最終 shared analysis 架構

### 原因

否則你會同時打開兩個最高風險工程：
- reconstruction path
- native stereo runtime

這樣最容易失控。

---

## 17. Claude 的實作規則

這份計畫給 Claude 實作時，請遵守以下規則：

1. 一次只做一個 phase
2. 每個 phase 都要先讓測試全綠
3. `LegacyAdditive` 不得提早刪除
4. 若某一步導致你主觀聽感明顯退化，先停，不要硬往下
5. 每個 phase 完成後都要留下：
   - code summary
   - tests summary
   - listening risk note

---

## 18. 建議 commit 節奏

### Commit 1
`refactor: introduce frame and hop runtime contracts`

### Commit 2
`refactor: add frame accumulator and zero-alloc stft scaffolding`

### Commit 3
`refactor: split m5 validator and synthesis responsibilities`

### Commit 4
`feat: add fft ola pilot synthesis mode`

### Commit 5
`feat: enable hop-aligned istft overlap-add reconstruction`

### Commit 6
`tune: retune style and stereo behavior for fft reconstruction`

---

## 19. 每個 phase 的測試要求

### 結構測試

- `cargo test -p asce-dsp-core -p asce-wasm-bridge`
- 新增 frame/hop/OLA 單元測試

### 穩定性測試

- 小 block
- 大 block
- 不同 sample rate
- silence / DC / clipped / low-cutoff

### 聽感測試

每次至少回聽以下類型：

- 明顯有效的 reference 歌
- 本來差異不大的歌
- 人聲主導素材
- 空間感明顯素材
- 低頻 impact 敏感素材

---

## 20. 停損條件

若出現以下任一情況，先停在當前 phase：

1. singer 明顯不再站前面
2. center image 漂移或變散
3. 原本的高級感變成一般 EQ 感
4. 低頻變糊或 punch 掉太多
5. browser 路徑穩定性變差

---

## 21. 一句話總結

`這個 FFT 分支不是為了證明數學比較強，而是要在不丟掉 Phaselith 現有魔法的前提下，逐步把主重建路徑升級成真正能長大的架構。`
