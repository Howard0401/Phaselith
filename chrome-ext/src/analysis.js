function formatSignedDb(value, digits = 2) {
  if (!Number.isFinite(value)) return '--';
  const sign = value > 0 ? '+' : '';
  return `${sign}${value.toFixed(digits)} dB`;
}

function formatDb(value, digits = 1) {
  if (!Number.isFinite(value)) return '--';
  return `${value.toFixed(digits)} dB`;
}

function formatCorr(value) {
  if (!Number.isFinite(value)) return '--';
  return value.toFixed(3);
}

function formatPct(value, digits = 1) {
  if (!Number.isFinite(value)) return '--';
  return `${value.toFixed(digits)}%`;
}

function formatSignedNumber(value, digits = 1, suffix = '') {
  if (!Number.isFinite(value)) return '--';
  const sign = value > 0 ? '+' : '';
  return `${sign}${value.toFixed(digits)}${suffix}`;
}

function classifySnapshot(snapshot) {
  if (!snapshot?.enoughData) {
    return {
      title: 'Collecting...',
      hint: 'Need a few seconds of active audio before the comparison stabilizes.',
    };
  }

  const dynStable = Math.abs(snapshot.dynRangeDeltaDb) <= 0.5;
  const crestStable = Math.abs(snapshot.crestDeltaDb) <= 0.6;
  const envHigh = snapshot.envelopeCorrelation >= 0.985;
  const wavePhaseShift =
    envHigh && (snapshot.waveCorrMean < 0.97 || snapshot.lagAbsMeanSamples >= 1.0);
  const roughnessRise = snapshot.roughnessDeltaMeanDb >= 0.8;
  const widthShift = Math.abs(snapshot.widthDeltaMeanDb) >= 0.8;
  const centerDrift = snapshot.balanceAbsDeltaMeanDb >= 0.35;
  const spaceShift = Math.abs(snapshot.spaceDeltaMeanPct) >= 2.5 || Math.abs(snapshot.earlyReflDeltaMeanPct) >= 3.0;

  if (dynStable && crestStable && envHigh && snapshot.residualMeanDb <= -35) {
    return {
      title: 'Near-transparent',
      hint: 'Envelope and transient structure are almost unchanged; the DSP is barely altering this passage.',
    };
  }

  if (
    snapshot.dynRangeDeltaDb <= -1.0
    || snapshot.crestDeltaDb <= -0.9
    || (snapshot.envelopeCorrelation < 0.965 && snapshot.dynRangeDeltaDb < -0.5)
  ) {
    return {
      title: 'Compression likely',
      hint: 'The output envelope range or crest factor is materially smaller than the input.',
    };
  }

  if (dynStable && crestStable && envHigh) {
    if (wavePhaseShift && roughnessRise) {
      return {
        title: 'Phase + roughness likely',
        hint: 'Envelope is preserved, but the waveform changed enough to suggest phase/timing shifts plus extra edge or grit.',
      };
    }
    if (wavePhaseShift) {
      return {
        title: 'Phase / timing likely',
        hint: 'Envelope is stable but waveform correlation dropped, which fits phase/timing or stereo-structure changes.',
      };
    }
    if (roughnessRise) {
      return {
        title: 'Distortion / edge likely',
        hint: 'Envelope is stable, but roughness rose. That fits added grit, distortion-like edge, or resonant bite.',
      };
    }
    if (widthShift || centerDrift || spaceShift) {
      return {
        title: 'Spatial image changed',
        hint: 'Macro dynamics are stable, but stereo width, center lock, or ambience proxies moved enough to change the presentation.',
      };
    }
    return {
      title: 'Tone / resonance likely',
      hint: 'Envelope shape is preserved, so perceived flattening is more likely from timbre, resonance, or phase changes.',
    };
  }

  return {
    title: 'Mixed / unclear',
    hint: 'There is a measurable change, but it is not a clean “true compression” or “pure tone illusion” case.',
  };
}

function drawSeries(canvas, yMin, yMax, lines) {
  const ctx = canvas.getContext('2d');
  const { width, height } = canvas;
  ctx.clearRect(0, 0, width, height);

  ctx.fillStyle = '#111423';
  ctx.fillRect(0, 0, width, height);

  ctx.strokeStyle = 'rgba(255,255,255,0.08)';
  ctx.lineWidth = 1;
  for (let i = 0; i <= 4; i++) {
    const y = 16 + ((height - 32) * i) / 4;
    ctx.beginPath();
    ctx.moveTo(40, y);
    ctx.lineTo(width - 12, y);
    ctx.stroke();
  }

  const plotWidth = width - 52;
  const plotHeight = height - 32;
  const range = Math.max(yMax - yMin, 1e-6);

  for (const line of lines) {
    if (!line.values?.length) continue;
    ctx.strokeStyle = line.color;
    ctx.lineWidth = 2;
    ctx.beginPath();
    line.values.forEach((value, index) => {
      const x = 40 + (plotWidth * index) / Math.max(line.values.length - 1, 1);
      const y = 16 + plotHeight - ((value - yMin) / range) * plotHeight;
      if (index === 0) {
        ctx.moveTo(x, y);
      } else {
        ctx.lineTo(x, y);
      }
    });
    ctx.stroke();
  }

  ctx.fillStyle = '#8d8da8';
  ctx.font = '11px Segoe UI';
  ctx.fillText(yMax.toFixed(1), 8, 20);
  ctx.fillText(yMin.toFixed(1), 8, height - 12);
}

function getSeriesBounds(...seriesList) {
  const values = seriesList.flat().filter(Number.isFinite);
  if (!values.length) {
    return { min: -1, max: 1 };
  }
  let min = Math.min(...values);
  let max = Math.max(...values);
  if (Math.abs(max - min) < 1.0) {
    min -= 0.5;
    max += 0.5;
  }
  return { min, max };
}

function meanOf(values) {
  const clean = values.filter(Number.isFinite);
  if (!clean.length) return NaN;
  return clean.reduce((sum, value) => sum + value, 0) / clean.length;
}

async function loadSnapshot() {
  const response = await chrome.runtime.sendMessage({ type: 'GET_ANALYSIS_SNAPSHOT' });
  return response?.payload || null;
}

function renderSnapshot(snapshot) {
  const emptyState = document.getElementById('emptyState');
  const content = document.getElementById('content');
  const subtitle = document.getElementById('subtitle');

  if (!snapshot || !snapshot.historyCount) {
    emptyState.style.display = 'block';
    content.style.display = 'none';
    return;
  }

  emptyState.style.display = 'none';
  content.style.display = 'block';

  subtitle.textContent =
    `Window ${snapshot.windowMs.toFixed(0)} ms, history ${snapshot.historySeconds.toFixed(1)} s, ${snapshot.historyCount} points`;

  document.getElementById('dynDelta').textContent = formatSignedDb(snapshot.dynRangeDeltaDb);
  document.getElementById('crestDelta').textContent = formatSignedDb(snapshot.crestDeltaDb);
  document.getElementById('envCorr').textContent = formatCorr(snapshot.envelopeCorrelation);
  document.getElementById('residualMean').textContent = formatDb(snapshot.residualMeanDb);
  document.getElementById('waveCorr').textContent = formatCorr(snapshot.waveCorrMean);
  document.getElementById('lagMean').textContent = Number.isFinite(snapshot.lagAbsMeanSamples)
    ? `${snapshot.lagAbsMeanSamples.toFixed(2)} smp`
    : '--';
  document.getElementById('roughnessDelta').textContent = formatSignedDb(snapshot.roughnessDeltaMeanDb);
  document.getElementById('widthDelta').textContent = formatSignedDb(snapshot.widthDeltaMeanDb);
  document.getElementById('inputWidth').textContent = formatDb(meanOf(snapshot.inputWidthDb), 2);
  document.getElementById('outputWidth').textContent = formatDb(meanOf(snapshot.outputWidthDb), 2);
  document.getElementById('centerDrift').textContent = formatDb(snapshot.balanceAbsDeltaMeanDb, 2);
  document.getElementById('spaceDelta').textContent = formatSignedNumber(snapshot.spaceDeltaMeanPct, 1, ' pt');
  document.getElementById('earlyReflDelta').textContent = formatSignedNumber(snapshot.earlyReflDeltaMeanPct, 1, ' pt');

  const reading = classifySnapshot(snapshot);
  document.getElementById('reading').textContent = reading.title;
  document.getElementById('readingHint').textContent = reading.hint;

  const rmsBounds = getSeriesBounds(snapshot.inputRmsDb, snapshot.outputRmsDb);
  drawSeries(
    document.getElementById('rmsCanvas'),
    rmsBounds.min - 1,
    rmsBounds.max + 1,
    [
      { values: snapshot.inputRmsDb, color: '#5eead4' },
      { values: snapshot.outputRmsDb, color: '#38bdf8' },
    ]
  );

  const crestBounds = getSeriesBounds(snapshot.inputCrestDb, snapshot.outputCrestDb);
  drawSeries(
    document.getElementById('crestCanvas'),
    crestBounds.min - 0.5,
    crestBounds.max + 0.5,
    [
      { values: snapshot.inputCrestDb, color: '#f59e0b' },
      { values: snapshot.outputCrestDb, color: '#f43f5e' },
    ]
  );

  const residualBounds = getSeriesBounds(snapshot.residualDb);
  drawSeries(
    document.getElementById('residualCanvas'),
    residualBounds.min - 1,
    residualBounds.max + 1,
    [
      { values: snapshot.residualDb, color: '#a78bfa' },
    ]
  );

  const phaseBounds = getSeriesBounds(
    snapshot.waveCorr
  );
  const phaseMin = Math.max(-1, phaseBounds.min - 0.02);
  const phaseMax = Math.min(1, phaseBounds.max + 0.02);
  drawSeries(
    document.getElementById('phaseCanvas'),
    phaseMin,
    phaseMax,
    [
      { values: snapshot.waveCorr, color: '#22c55e' },
    ]
  );

  const roughnessBounds = getSeriesBounds(snapshot.roughnessDeltaDb);
  drawSeries(
    document.getElementById('roughnessCanvas'),
    roughnessBounds.min - 1,
    roughnessBounds.max + 1,
    [
      { values: snapshot.roughnessDeltaDb, color: '#f97316' },
    ]
  );

  const widthBounds = getSeriesBounds(snapshot.inputWidthDb, snapshot.outputWidthDb);
  drawSeries(
    document.getElementById('widthCanvas'),
    widthBounds.min - 0.5,
    widthBounds.max + 0.5,
    [
      { values: snapshot.inputWidthDb, color: '#60a5fa' },
      { values: snapshot.outputWidthDb, color: '#c084fc' },
    ]
  );

  const spaceBounds = getSeriesBounds(snapshot.inputSpacePct, snapshot.outputSpacePct);
  drawSeries(
    document.getElementById('spaceCanvas'),
    Math.max(0, spaceBounds.min - 1),
    spaceBounds.max + 1,
    [
      { values: snapshot.inputSpacePct, color: '#34d399' },
      { values: snapshot.outputSpacePct, color: '#f472b6' },
    ]
  );

  const earlyBounds = getSeriesBounds(snapshot.inputEarlyReflPct, snapshot.outputEarlyReflPct);
  drawSeries(
    document.getElementById('earlyCanvas'),
    Math.max(0, earlyBounds.min - 1),
    earlyBounds.max + 1,
    [
      { values: snapshot.inputEarlyReflPct, color: '#fbbf24' },
      { values: snapshot.outputEarlyReflPct, color: '#ef4444' },
    ]
  );
}

async function refreshSnapshot() {
  try {
    await chrome.runtime.sendMessage({ type: 'REQUEST_ANALYSIS_SNAPSHOT_NOW' });
  } catch {
    // Ignore races during reload.
  }
  setTimeout(async () => {
    const snapshot = await loadSnapshot();
    renderSnapshot(snapshot);
  }, 350);
}

document.getElementById('refreshBtn').addEventListener('click', refreshSnapshot);
refreshSnapshot();
setInterval(refreshSnapshot, 1000);
