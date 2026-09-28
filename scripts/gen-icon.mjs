// 生成最简 32x32 ICO（Indigo 纯色占位图标）—— 替换正式图标时用设计稿导出覆盖
import { writeFileSync, mkdirSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const out = join(dirname(fileURLToPath(import.meta.url)), '..', 'src-tauri', 'icons', 'icon.ico')
mkdirSync(dirname(out), { recursive: true })

const W = 32, H = 32
const header = Buffer.alloc(6)
header.writeUInt16LE(0, 0) // reserved
header.writeUInt16LE(1, 2) // type: icon
header.writeUInt16LE(1, 4) // count

// BITMAPINFOHEADER（高度翻倍：XOR+AND）
const bih = Buffer.alloc(40)
bih.writeUInt32LE(40, 0)
bih.writeInt32LE(W, 4)
bih.writeInt32LE(H * 2, 8)
bih.writeUInt16LE(1, 12)
bih.writeUInt16LE(32, 14)
bih.writeUInt32LE(0, 16)
bih.writeUInt32LE(W * H * 4, 20)

// XOR（BGRA，自下而上）：Indigo #5F60EF，中心 20x32 白色 "S" 竖条示意
const xor = Buffer.alloc(W * H * 4)
for (let y = 0; y < H; y++) {
  for (let x = 0; x < W; x++) {
    const row = H - 1 - y
    const i = (row * W + x) * 4
    const inBar = x >= 6 && x < 26 && y >= 6 && y < 26
    xor[i] = inBar ? 0xff : 0xef
    xor[i + 1] = inBar ? 0xff : 0x60
    xor[i + 2] = inBar ? 0xff : 0x5f
    xor[i + 3] = 0xff
  }
}
// AND 掩码（全 0 = 不透明）
const and = Buffer.alloc((W / 8) * H)

const img = Buffer.concat([bih, xor, and])
const entry = Buffer.alloc(16)
entry.writeUInt8(W, 0)
entry.writeUInt8(H, 1)
entry.writeUInt16LE(1, 4)
entry.writeUInt16LE(32, 6)
entry.writeUInt32LE(img.length, 8)
entry.writeUInt32LE(22, 12)

writeFileSync(out, Buffer.concat([header, entry, img]))
console.log('icon written:', out)
