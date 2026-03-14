## Short Description (manifest, max 132 chars)

> Real-time browser audio optimization that reduces haze, improves focus, and strengthens center image.

---

## Detailed Description (Store listing)

CIRRUS is a real-time browser audio optimization engine. It analyzes playback in real time and helps reduce haze, roughness, and image instability so voices and instruments feel more focused, more intelligible, and more physically placed in front of the listener.

Unlike EQ plugins or sound enhancers that apply fixed tonal curves, CIRRUS uses a multi-stage signal analysis pipeline to detect degradation patterns, generate candidate repairs, and mix only bounded, validated changes back into the signal.

### How It Works

CIRRUS processes audio through a 6-stage restoration pipeline:

1. Damage Detection — Identifies codec cutoff frequency, clipping artifacts, dynamic compression, and stereo collapse.
2. Signal Decomposition — Separates the audio into harmonic, air, transient, and spatial components.
3. Residual Synthesis — Generates candidate repairs for each damaged component using physical models (not neural networks).
4. Self-Validation — Candidate repairs are checked against a consistency model. Repairs that fail the check are reduced or rejected before mixing.
5. Perceptual Mixing — Validated repairs are mixed back at safe levels with loudness compensation and peak protection.
6. Ambience Preservation — Reverb tails and spatial cues are preserved, not destroyed.

### Key Features

- Low-latency real-time processing for browser playback
- Works on any browser audio: music, podcasts, video calls, and more
- 7 style presets: Reference, Grand, Smooth, Vocal, Punch, Air, Night
- Adjustable strength, HF reconstruction, and dynamics controls
- Dual-mono stereo processing (independent L/R channels)
- Runs entirely on CPU via WebAssembly — no GPU required
- Zero data collection, no account needed

### What Makes CIRRUS Different

Most audio enhancers add bass boost, virtual surround, or EQ curves on top of your audio. CIRRUS instead analyzes degradation patterns and applies bounded, validated repair candidates rather than a fixed tonal recipe.

### Who Is This For

- Music listeners who want richer sound from browser playback
- Podcast listeners who want clearer, more natural voices
- Headphone users who want to get more from their existing gear
- Anyone who notices that browser audio sounds flat compared to dedicated players

### Early Access

CIRRUS is currently in Early Access. The core algorithm is stable and producing strong results, but we're actively refining presets and adding features based on user feedback.

---

即時瀏覽器音訊優化 — 降低霧感、提升焦點，並強化中心結像。

CIRRUS 是一個在瀏覽器中即時運行的音訊優化引擎。它會即時分析播放內容，並協助降低霧感、粗糙感與結像不穩定，讓人聲與樂器更聚焦、更容易理解，也更像實際位於聆聽者前方發聲。

與套用固定音調曲線的 EQ 插件或音效增強器不同，CIRRUS 使用多階段訊號分析管線來偵測退化模式、生成候選修復，並只把有界、通過驗證的變更混回訊號。

### 運作原理

CIRRUS 透過 6 階段修復管線處理音訊：

1. 損傷偵測 — 識別編解碼器截止頻率、削波失真、動態壓縮和立體聲塌縮。
2. 訊號分解 — 將音訊分離為諧波、空氣感、瞬態和空間成分。
3. 殘差合成 — 使用物理模型（非神經網路）為每個受損成分生成候選修復。
4. 自我驗證 — 候選修復會經過一致性檢查。未通過的修復會在混音前被縮小或拒絕。
5. 感知混合 — 已驗證的修復以安全音量混合回去，搭配響度補償和峰值保護。
6. 氛圍保留 — 殘響尾音和空間線索被保留，不會被破壞。

### 主要特色

- 低延遲的即時瀏覽器播放處理
- 適用於任何瀏覽器音訊：音樂、播客、視訊通話等
- 7 種風格預設：參考、磅礡、溫潤、人聲、力道、空氣感、夜間
- 可調整強度、高頻重建和動態控制
- 雙單聲道立體聲處理（獨立左右聲道）
- 完全透過 WebAssembly 在 CPU 上運行 — 不需要 GPU
- 零資料收集，無需帳號

### CIRRUS 的獨特之處

大多數音訊增強器會在音訊上疊加低音增強、虛擬環繞或 EQ 曲線。CIRRUS 的做法不同：它分析退化模式，並加入有界、經過驗證的候選修復，而不是套用固定的音色配方。

### 適合誰

- 想從瀏覽器播放中獲得更豐富音質的音樂聽眾
- 想要更清晰、更自然聲音的播客聽眾
- 想從現有設備獲得更多的耳機使用者
- 任何注意到瀏覽器音訊聽起來比專用播放器平淡的人

### 早期預覽

CIRRUS 目前處於早期預覽階段。核心演算法已穩定並產出強勁結果，我們正在根據用戶回饋積極改進預設和新增功能。
