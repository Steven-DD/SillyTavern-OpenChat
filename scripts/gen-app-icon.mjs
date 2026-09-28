#!/usr/bin/env node
/**
 * 应用图标生成器（无第三方依赖）
 *
 * 产出 1024×1024 PNG（RGBA），配色取自设计 token 的 Indigo 主色族：
 *   底色渐变 #6D6EF3 → #4F46E5（p-400 → p-600 之间），白色对话气泡 + 三个 Indigo 圆点。
 *
 * 之后用 Tauri CLI 由它派生出全套平台图标：
 *   npx tauri icon src-tauri/icons/app-icon.png
 *
 * 为什么手写 PNG 编码：本机没有 ImageMagick / canvas，也不想为一张图标引入重依赖。
 * PNG 结构简单（IHDR + IDAT(zlib) + IEND），node 自带 zlib 足够。
 *
 * 用法：node scripts/gen-app-icon.mjs
 */
import zlib from 'node:zlib'
import { writeFileSync, mkdirSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const __dirname = path.dirname(fileURLToPath(import.meta.url))
const OUT_DIR = path.resolve(__dirname, '..', 'src-tauri', 'icons')
const OUT = path.join(OUT_DIR, 'app-icon.png')

/* ---------------- PNG 编码 ---------------- */

function crc32(buf) {
  let crc = 0xffffffff
  for (let i = 0; i < buf.length; i++) {
    let c = (crc ^ buf[i]) & 0xff
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1
    crc = c ^ (crc >>> 8)
  }
  return (crc ^ 0xffffffff) >>> 0
}

function chunk(type, data) {
  const len = Buffer.alloc(4)
  len.writeUInt32BE(data.length)
  const body = Buffer.concat([Buffer.from(type, 'ascii'), data])
  const crc = Buffer.alloc(4)
  crc.writeUInt32BE(crc32(body))
  return Buffer.concat([len, body, crc])
}

function encodePng(w, h, rgba) {
  const ihdr = Buffer.alloc(13)
  ihdr.writeUInt32BE(w, 0)
  ihdr.writeUInt32BE(h, 4)
  ihdr[8] = 8 // bit depth
  ihdr[9] = 6 // color type: RGBA
  ihdr[10] = 0
  ihdr[11] = 0
  ihdr[12] = 0

  const stride = w * 4
  const raw = Buffer.alloc((stride + 1) * h)
  for (let y = 0; y < h; y++) {
    raw[y * (stride + 1)] = 0 // filter: none
    rgba.copy(raw, y * (stride + 1) + 1, y * stride, (y + 1) * stride)
  }

  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', ihdr),
    chunk('IDAT', zlib.deflateSync(raw, { level: 9 })),
    chunk('IEND', Buffer.alloc(0)),
  ])
}

/* ---------------- 几何 ---------------- */

/** 圆角矩形有符号距离（<0 在内部） */
function sdRoundRect(px, py, cx, cy, hw, hh, r) {
  const dx = Math.abs(px - cx) - (hw - r)
  const dy = Math.abs(py - cy) - (hh - r)
  const ax = Math.max(dx, 0)
  const ay = Math.max(dy, 0)
  return Math.hypot(ax, ay) + Math.min(Math.max(dx, dy), 0) - r
}

/** 点在三角形内（重心符号法） */
function inTriangle(px, py, a, b, c) {
  const sign = (p1, p2, p3) =>
    (p1[0] - p3[0]) * (p2[1] - p3[1]) - (p2[0] - p3[0]) * (p1[1] - p3[1])
  const d1 = sign([px, py], a, b)
  const d2 = sign([px, py], b, c)
  const d3 = sign([px, py], c, a)
  const neg = d1 < 0 || d2 < 0 || d3 < 0
  const pos = d1 > 0 || d2 > 0 || d3 > 0
  return !(neg && pos)
}

/* ---------------- 渲染 ---------------- */

const SIZE = 1024
const SS = 3 // 超采样倍数：先 3 倍渲染再降采样，得到平滑边缘
const HI = SIZE * SS

const lerp = (a, b, t) => a + (b - a) * t

// 配色（对齐 tokens.css 的 Indigo 主色族）
const TOP = [0x6d, 0x6e, 0xf3]
const BOTTOM = [0x4f, 0x46, 0xe5]
const DOT = [0x4f, 0x46, 0xe5]
const WHITE = [0xff, 0xff, 0xff]

const P = SIZE / 1024 // 几何用 1024 基准坐标书写，便于肉眼校对
const bgR = 230 * P
// 气泡主体
const bx = 512 * P
const by = 468 * P
const bhw = 268 * P
const bhh = 168 * P
const br = 92 * P
// 尾巴（左下三角，与气泡底部相接）
const tail = [
  [318 * P, 596 * P],
  [352 * P, 800 * P],
  [498 * P, 620 * P],
]
// 三个圆点（typing 指示）
const dotR = 38 * P
const dotY = by
const dots = [bx - 96 * P, bx, bx + 96 * P]

const hi = new Uint8Array(HI * HI * 4)

for (let y = 0; y < HI; y++) {
  const py = y / SS
  for (let x = 0; x < HI; x++) {
    const px = x / SS
    let r = 0
    let g = 0
    let b = 0
    let a = 0

    // 1) 圆角方形底 + 竖向渐变
    if (sdRoundRect(px, py, 512 * P, 512 * P, 512 * P, 512 * P, bgR) <= 0) {
      const t = Math.min(1, Math.max(0, py / (1024 * P)))
      r = lerp(TOP[0], BOTTOM[0], t)
      g = lerp(TOP[1], BOTTOM[1], t)
      b = lerp(TOP[2], BOTTOM[2], t)
      a = 255
    }

    // 2) 白色气泡（主体 ∪ 尾巴）
    const inBubble =
      sdRoundRect(px, py, bx, by, bhw, bhh, br) <= 0 || inTriangle(px, py, ...tail)
    if (inBubble) {
      r = WHITE[0]
      g = WHITE[1]
      b = WHITE[2]
      a = 255
    }

    // 3) 三个圆点
    for (const dx of dots) {
      if (Math.hypot(px - dx, py - dotY) <= dotR) {
        r = DOT[0]
        g = DOT[1]
        b = DOT[2]
        a = 255
        break
      }
    }

    const o = (y * HI + x) * 4
    hi[o] = r
    hi[o + 1] = g
    hi[o + 2] = b
    hi[o + 3] = a
  }
}

// 降采样（盒式滤波，兼顾 RGB 与 alpha）
const out = Buffer.alloc(SIZE * SIZE * 4)
const n = SS * SS
for (let y = 0; y < SIZE; y++) {
  for (let x = 0; x < SIZE; x++) {
    let r = 0
    let g = 0
    let b = 0
    let a = 0
    for (let sy = 0; sy < SS; sy++) {
      for (let sx = 0; sx < SS; sx++) {
        const o = ((y * SS + sy) * HI + (x * SS + sx)) * 4
        r += hi[o]
        g += hi[o + 1]
        b += hi[o + 2]
        a += hi[o + 3]
      }
    }
    const o = (y * SIZE + x) * 4
    out[o] = Math.round(r / n)
    out[o + 1] = Math.round(g / n)
    out[o + 2] = Math.round(b / n)
    out[o + 3] = Math.round(a / n)
  }
}

mkdirSync(OUT_DIR, { recursive: true })
const png = encodePng(SIZE, SIZE, out)
writeFileSync(OUT, png)
console.log(`✅ 已生成 ${OUT}（${SIZE}×${SIZE}, ${(png.length / 1024).toFixed(1)} KB）`)
console.log(`   下一步：npx tauri icon "${path.relative(process.cwd(), OUT).replace(/\\/g, '/')}"`)
