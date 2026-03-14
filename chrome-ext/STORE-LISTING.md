## Short Description (manifest, max 132 chars)

> Real-time audio restoration engine — recovers lost harmonics, air, and detail from compressed browser audio.

---

## Detailed Description (Store listing)

CIRRUS is a real-time audio restoration engine that runs directly in your browser. It analyzes and repairs the perceptual damage caused by lossy compression — recovering lost harmonics, restoring high-frequency air, and rebuilding the spatial detail that codecs strip away.

Unlike EQ plugins or sound enhancers that apply fixed tonal curves, CIRRUS uses a multi-stage signal analysis pipeline to detect what's actually missing from your audio and reconstruct it in real time.

### How It Works

CIRRUS processes audio through a 6-stage restoration pipeline:

1. Damage Detection — Identifies codec cutoff frequency, clipping artifacts, dynamic compression, and stereo collapse.
2. Signal Decomposition — Separates the audio into harmonic, air, transient, and spatial components.
3. Residual Synthesis — Generates candidate repairs for each damaged component using physical models (not neural networks).
4. Self-Validation — Every repair is tested against the original signal. If a repair doesn't pass the consistency check, it's rejected. This means CIRRUS never makes your audio worse.
5. Perceptual Mixing — Validated repairs are mixed back at safe levels with loudness compensation and peak protection.
6. Ambience Preservation — Reverb tails and spatial cues are preserved, not destroyed.

### Key Features

- Real-time processing with under 3ms latency
- Works on any browser audio: music, podcasts, video calls, and more
- 7 style presets: Reference, Grand, Smooth, Vocal, Punch, Air, Night
- Adjustable strength, HF reconstruction, and dynamics controls
- Dual-mono stereo processing (independent L/R channels)
- Runs entirely on CPU via WebAssembly — no GPU required
- Zero data collection, no account needed, fully offline processing

### What Makes CIRRUS Different

Most audio enhancers add bass boost, virtual surround, or EQ curves on top of your audio. CIRRUS does the opposite — it analyzes what's been lost and puts it back. The self-validation step (M5 reprojection) ensures that every modification is verified before it reaches your ears. No other real-time audio tool does this.

### Who Is This For

- Music listeners who want richer sound from browser playback
- Podcast listeners who want clearer, more natural voices
- Headphone users who want to get more from their existing gear
- Anyone who notices that browser audio sounds flat compared to dedicated players

### Early Access

CIRRUS is currently in Early Access. The core algorithm is stable and producing strong results, but we're actively refining presets and adding features based on user feedback.

---

即時音訊修復引擎 — 還原壓縮音訊中遺失的諧波、空氣感與細節。

CIRRUS 是一個在瀏覽器中即時運行的音訊修復引擎。它分析並修復有損壓縮造成的感知損傷 — 還原遺失的諧波、恢復高頻空氣感，並重建被編解碼器剝離的空間細節。

與套用固定音調曲線的 EQ 插件或音效增強器不同，CIRRUS 使用多階段訊號分析管線來偵測音訊中實際缺失的部分，並即時重建。

### 運作原理

CIRRUS 透過 6 階段修復管線處理音訊：

1. 損傷偵測 — 識別編解碼器截止頻率、削波失真、動態壓縮和立體聲塌縮。
2. 訊號分解 — 將音訊分離為諧波、空氣感、瞬態和空間成分。
3. 殘差合成 — 使用物理模型（非神經網路）為每個受損成分生成候選修復。
4. 自我驗證 — 每個修復都會對照原始訊號進行測試。未通過一致性檢查的修復會被拒絕。這意味著 CIRRUS 絕不會讓你的音訊變差。
5. 感知混合 — 已驗證的修復以安全音量混合回去，搭配響度補償和峰值保護。
6. 氛圍保留 — 殘響尾音和空間線索被保留，不會被破壞。

### 主要特色

- 即時處理，延遲低於 3 毫秒
- 適用於任何瀏覽器音訊：音樂、播客、視訊通話等
- 7 種風格預設：參考、磅礡、溫潤、人聲、力道、空氣感、夜間
- 可調整強度、高頻重建和動態控制
- 雙單聲道立體聲處理（獨立左右聲道）
- 完全透過 WebAssembly 在 CPU 上運行 — 不需要 GPU
- 零資料收集，無需帳號，完全離線處理

### CIRRUS 的獨特之處

大多數音訊增強器在音訊上疊加低音增強、虛擬環繞或 EQ 曲線。CIRRUS 反其道而行 — 它分析遺失了什麼，然後放回去。自我驗證步驟（M5 重投影）確保每個修改在到達你耳朵之前都經過驗證。沒有其他即時音訊工具做到這一點。

### 適合誰

- 想從瀏覽器播放中獲得更豐富音質的音樂聽眾
- 想要更清晰、更自然聲音的播客聽眾
- 想從現有設備獲得更多的耳機使用者
- 任何注意到瀏覽器音訊聽起來比專用播放器平淡的人

### 早期預覽

CIRRUS 目前處於早期預覽階段。核心演算法已穩定並產出強勁結果，我們正在根據用戶回饋積極改進預設和新增功能。
