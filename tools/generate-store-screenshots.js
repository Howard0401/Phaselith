#!/usr/bin/env node
/**
 * Phaselith - Chrome Web Store Screenshot Generator
 *
 * Generates promotional screenshots for Chrome Web Store listing.
 * Output: 1280x800 24-bit PNG (no alpha) with branded background.
 *
 * Usage:
 *   node tools/generate-store-screenshots.js <input-png> [--output-dir <dir>]
 *
 * Examples:
 *   node tools/generate-store-screenshots.js screenshot.png
 *   node tools/generate-store-screenshots.js popup.png --output-dir store-assets
 *
 * Requirements:
 *   npm install sharp
 */

const sharp = require('sharp');
const path = require('path');
const fs = require('fs');

// --- Config ---
const STORE_WIDTH = 1280;
const STORE_HEIGHT = 800;
const BG_COLOR_TOP = { r: 15, g: 15, b: 26 };     // #0f0f1a
const BG_COLOR_BOT = { r: 22, g: 22, b: 48 };     // #161630
const ACCENT_CYAN = '#00d4ff';
const ACCENT_PURPLE = '#7b68ee';
const TAGLINE_EN = 'Real-Time Audio Enhancement';
const TAGLINE_ZH = '即時音訊強化引擎';

async function generateGradientBg(width, height) {
  // Create vertical gradient background via SVG
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}">
    <defs>
      <linearGradient id="bg" x1="0" y1="0" x2="0" y2="1">
        <stop offset="0%" stop-color="rgb(${BG_COLOR_TOP.r},${BG_COLOR_TOP.g},${BG_COLOR_TOP.b})"/>
        <stop offset="100%" stop-color="rgb(${BG_COLOR_BOT.r},${BG_COLOR_BOT.g},${BG_COLOR_BOT.b})"/>
      </linearGradient>
      <radialGradient id="glow1" cx="30%" cy="40%">
        <stop offset="0%" stop-color="${ACCENT_CYAN}" stop-opacity="0.08"/>
        <stop offset="100%" stop-color="${ACCENT_CYAN}" stop-opacity="0"/>
      </radialGradient>
      <radialGradient id="glow2" cx="70%" cy="60%">
        <stop offset="0%" stop-color="${ACCENT_PURPLE}" stop-opacity="0.06"/>
        <stop offset="100%" stop-color="${ACCENT_PURPLE}" stop-opacity="0"/>
      </radialGradient>
    </defs>
    <rect width="${width}" height="${height}" fill="url(#bg)"/>
    <rect width="${width}" height="${height}" fill="url(#glow1)"/>
    <rect width="${width}" height="${height}" fill="url(#glow2)"/>
  </svg>`;
  return sharp(Buffer.from(svg)).png().toBuffer();
}

async function generateTaglineOverlay(width, height, tagline) {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}">
    <defs>
      <linearGradient id="textGrad" x1="0" y1="0" x2="1" y2="0">
        <stop offset="0%" stop-color="${ACCENT_CYAN}"/>
        <stop offset="100%" stop-color="${ACCENT_PURPLE}"/>
      </linearGradient>
    </defs>
    <text x="${width / 2}" y="80" text-anchor="middle"
          font-family="Inter, Segoe UI, system-ui, sans-serif"
          font-size="42" font-weight="700" letter-spacing="2"
          fill="url(#textGrad)">${tagline}</text>
  </svg>`;
  return sharp(Buffer.from(svg)).png().toBuffer();
}

async function main() {
  const args = process.argv.slice(2);

  if (args.length === 0 || args.includes('--help')) {
    console.log(`
Phaselith - Store Screenshot Generator

Usage:
  node tools/generate-store-screenshots.js <input-png> [options]

Options:
  --output-dir <dir>   Output directory (default: store-assets)
  --tagline <text>     Custom tagline text
  --scale <factor>     Scale factor for popup image (default: auto-fit)
  --help               Show this help

Output:
  1280x800 24-bit PNG (no alpha) - Chrome Web Store format
`);
    process.exit(0);
  }

  const inputFile = args[0];
  if (!fs.existsSync(inputFile)) {
    console.error(`Error: Input file not found: ${inputFile}`);
    process.exit(1);
  }

  // Parse options
  let outputDir = 'store-assets';
  let customTagline = null;
  let customScale = null;

  for (let i = 1; i < args.length; i++) {
    if (args[i] === '--output-dir' && args[i + 1]) {
      outputDir = args[++i];
    } else if (args[i] === '--tagline' && args[i + 1]) {
      customTagline = args[++i];
    } else if (args[i] === '--scale' && args[i + 1]) {
      customScale = parseFloat(args[++i]);
    }
  }

  fs.mkdirSync(outputDir, { recursive: true });

  // Get input image metadata
  const inputMeta = await sharp(inputFile).metadata();
  console.log(`Input: ${inputFile} (${inputMeta.width}x${inputMeta.height})`);

  // Calculate scale to fit popup in the canvas with padding
  const maxPopupHeight = STORE_HEIGHT - 160; // top/bottom padding
  const maxPopupWidth = STORE_WIDTH - 200;
  const scale = customScale || Math.min(
    maxPopupHeight / inputMeta.height,
    maxPopupWidth / inputMeta.width,
    2.0 // max 2x upscale
  );

  const scaledW = Math.round(inputMeta.width * scale);
  const scaledH = Math.round(inputMeta.height * scale);
  console.log(`Scale: ${scale.toFixed(2)}x -> ${scaledW}x${scaledH}`);

  // Generate background
  const bgBuffer = await generateGradientBg(STORE_WIDTH, STORE_HEIGHT);

  // Scale popup screenshot
  const popupBuffer = await sharp(inputFile)
    .resize(scaledW, scaledH, { kernel: 'lanczos3' })
    .png()
    .toBuffer();

  // Add rounded corners + drop shadow to popup
  const cornerRadius = 16;
  const shadowSize = 20;
  const popupWithShadow = await sharp({
    create: {
      width: scaledW + shadowSize * 2,
      height: scaledH + shadowSize * 2,
      channels: 4,
      background: { r: 0, g: 0, b: 0, alpha: 0 }
    }
  })
    .composite([
      // Shadow layer
      {
        input: await sharp(Buffer.from(
          `<svg width="${scaledW + shadowSize * 2}" height="${scaledH + shadowSize * 2}">
            <defs><filter id="shadow"><feGaussianBlur stdDeviation="${shadowSize / 2}"/></filter></defs>
            <rect x="${shadowSize}" y="${shadowSize}" width="${scaledW}" height="${scaledH}" rx="${cornerRadius}"
                  fill="rgba(0,0,0,0.5)" filter="url(#shadow)"/>
          </svg>`
        )).png().toBuffer(),
        top: 0, left: 0
      },
      // Popup with rounded mask
      {
        input: await sharp(popupBuffer)
          .composite([{
            input: Buffer.from(
              `<svg width="${scaledW}" height="${scaledH}">
                <rect width="${scaledW}" height="${scaledH}" rx="${cornerRadius}" fill="white"/>
              </svg>`
            ),
            blend: 'dest-in'
          }])
          .png()
          .toBuffer(),
        top: shadowSize, left: shadowSize
      }
    ])
    .png()
    .toBuffer();

  // Center position for popup on canvas
  const popupLeft = Math.round((STORE_WIDTH - scaledW - shadowSize * 2) / 2);
  const popupTop = Math.round((STORE_HEIGHT - scaledH - shadowSize * 2) / 2) + 30; // offset down for tagline

  // Generate tagline
  const tagline = customTagline || TAGLINE_EN;
  const taglineBuffer = await generateTaglineOverlay(STORE_WIDTH, STORE_HEIGHT, tagline);

  // Compose final image
  const finalBuffer = await sharp(bgBuffer)
    .composite([
      { input: taglineBuffer, top: 0, left: 0 },
      { input: popupWithShadow, top: popupTop, left: popupLeft }
    ])
    .flatten({ background: BG_COLOR_TOP }) // Remove alpha -> 24-bit PNG
    .png()
    .toBuffer();

  // Save
  const outputPath = path.join(outputDir, 'store-screenshot-en.png');
  await sharp(finalBuffer).toFile(outputPath);
  const stats = fs.statSync(outputPath);
  console.log(`Saved: ${outputPath} (${(stats.size / 1024).toFixed(0)} KB)`);

  // Also generate Chinese version
  const taglineZhBuffer = await generateTaglineOverlay(STORE_WIDTH, STORE_HEIGHT, TAGLINE_ZH);
  const finalZhBuffer = await sharp(bgBuffer)
    .composite([
      { input: taglineZhBuffer, top: 0, left: 0 },
      { input: popupWithShadow, top: popupTop, left: popupLeft }
    ])
    .flatten({ background: BG_COLOR_TOP })
    .png()
    .toBuffer();

  const outputZhPath = path.join(outputDir, 'store-screenshot-zh.png');
  await sharp(finalZhBuffer).toFile(outputZhPath);
  const statsZh = fs.statSync(outputZhPath);
  console.log(`Saved: ${outputZhPath} (${(statsZh.size / 1024).toFixed(0)} KB)`);

  console.log(`\nDone! ${STORE_WIDTH}x${STORE_HEIGHT} 24-bit PNG (no alpha)`);
  console.log('Ready for Chrome Web Store upload.');
}

main().catch(err => {
  console.error('Error:', err.message);
  process.exit(1);
});
