#!/usr/bin/env python3
"""构造 FileClean 冒烟测试用的目录树（纯标准库，无第三方依赖）。

覆盖场景：
  1. 精确重复（同名不同目录 / 不同文件名）
  2. 同体积但内容不同（不应判为重复）
  3. 逐字节相同的图片（精确重复）
  4. 感知相似图片（同图改分辨率 + 调色，不应判为精确重复，但应被感知哈希命中）
  5. 无关图片（对照组，不应被合并）
  6. RAW + JPEG 配对
  7. ZIP 内部重复条目
  8. 隐藏文件（默认跳过）
  9. 硬链接（同 inode，不应计入可回收空间）
 10. 零字节文件（默认 min_size=1 跳过）
 11. 伪造 Lightroom 目录（引用完整性保护）
 12. 同一段音频的不同采样率（音频指纹）
 13. 完全不同的音频（对照组）

关于图片素材的说明（重要）：
    早期版本用 `(x*2) % 256` 这类**取模折叠**的渐变生成「相似图」，结果感知哈希
    距离高达 28 bit。原因是取模会制造锯齿断崖，且不同分辨率下折叠次数不同，
    频谱被彻底打乱 —— 这是病态输入，不是算法缺陷。
    现改用**多倍频 value noise（fbm）**：频谱接近自然图像，且定义在归一化坐标系上，
    因此「同一张图的不同分辨率」能得到 pHash 距离 0。

    另一处关键点：相似图必须**不是**精确重复。因为流水线会把已进入精确重复组的文件
    从相似检测中排除（避免同一堆文件被报两次），若相似图互为逐字节副本，
    它们会在精确阶段被消耗掉，感知阶段就再也看不到它们了。
"""

from __future__ import annotations

import math
import os
import shutil
import sqlite3
import struct
import sys
import zipfile
import zlib

ROOT = sys.argv[1] if len(sys.argv) > 1 else "/tmp/fileclean-fixture"


# --------------------------------------------------------------------------- #
# PNG 写入（纯标准库：zlib + struct）
# --------------------------------------------------------------------------- #
def write_png(path: str, width: int, height: int, pixel_fn) -> None:
    rows = []
    for y in range(height):
        row = bytearray()
        for x in range(width):
            r, g, b = pixel_fn(x, y)
            row += bytes((r, g, b))
        rows.append(b"\x00" + bytes(row))
    raw = b"".join(rows)

    def chunk(tag: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(raw, 6))
    png += chunk(b"IEND", b"")
    with open(path, "wb") as fh:
        fh.write(png)


# --------------------------------------------------------------------------- #
# 类真实图像：多倍频 value noise（决定论、分辨率无关）
# --------------------------------------------------------------------------- #
def _hash01(i: int, j: int, salt: int) -> float:
    """确定性伪随机数 [0, 1)，替代 random 以保证可复现。"""
    h = (i * 374761393 + j * 668265263 + salt * 2246822519) & 0xFFFFFFFF
    h = ((h ^ (h >> 13)) * 1274126177) & 0xFFFFFFFF
    return ((h ^ (h >> 16)) & 0xFFFF) / 65536.0


def _smoothstep(t: float) -> float:
    return t * t * (3.0 - 2.0 * t)


def value_noise(u: float, v: float, cells: int, salt: int) -> float:
    """双线性插值的 value noise，定义域为归一化坐标。"""
    x, y = u * cells, v * cells
    xi, yi = int(x), int(y)
    xf, yf = _smoothstep(x - xi), _smoothstep(y - yi)
    c00 = _hash01(xi, yi, salt)
    c10 = _hash01(xi + 1, yi, salt)
    c01 = _hash01(xi, yi + 1, salt)
    c11 = _hash01(xi + 1, yi + 1, salt)
    top = c00 + (c10 - c00) * xf
    bot = c01 + (c11 - c01) * xf
    return top + (bot - top) * yf


def fbm(u: float, v: float, octaves: int = 6, salt: int = 1) -> float:
    """多倍频叠加，1/f 衰减 —— 频谱丰富度接近自然图像。"""
    total = 0.0
    amp = 1.0
    norm = 0.0
    for o in range(octaves):
        total += amp * value_noise(u, v, 3 * (2**o), salt + o * 17)
        norm += amp
        amp *= 0.5
    return total / norm


def scene(seed: int, bright: float = 0.0):
    """返回一个 (x, y, w, h) -> (r, g, b) 的像素函数。

    同一 seed 在不同分辨率下渲染出的是「同一张图」，因此在感知哈希下距离应接近 0。
    """
    def pixel(x: int, y: int, w: int, h: int) -> tuple[int, int, int]:
        u, v = x / (w - 1), y / (h - 1)
        base = fbm(u, v, 6, seed)
        r = 40 + 190 * fbm(u, v, 5, seed + 101)
        g = 40 + 190 * fbm(u, v, 5, seed + 211)
        b = 40 + 190 * (0.5 * fbm(u, v, 5, seed + 307) + 0.5 * base)
        # 叠一层硬边结构（模拟物体轮廓），进一步提升频谱丰富度
        edge = (math.floor(u * 7) + math.floor(v * 5)) % 2
        r = r * 0.85 + edge * 25
        clamp = lambda c: max(0, min(255, int(round(c + bright))))  # noqa: E731
        return clamp(r), clamp(g), clamp(b)

    return pixel


def write_scene(path: str, w: int, h: int, seed: int, bright: float = 0.0) -> None:
    fn = scene(seed, bright)
    write_png(path, w, h, lambda x, y: fn(x, y, w, h))


# --------------------------------------------------------------------------- #
# WAV 写入（单声道 16-bit PCM）
# --------------------------------------------------------------------------- #
def write_wav(path: str, seconds: float, sample_rate: int, freq_fn) -> None:
    n = int(seconds * sample_rate)
    frames = bytearray()
    phase = 0.0
    for i in range(n):
        t = i / sample_rate
        phase += 2.0 * math.pi * freq_fn(t) / sample_rate
        frames += struct.pack("<h", int(0.6 * 32767 * math.sin(phase)))

    data_size = len(frames)
    header = b"RIFF" + struct.pack("<I", 36 + data_size) + b"WAVE"
    header += b"fmt " + struct.pack("<IHHIIHH", 16, 1, 1, sample_rate, sample_rate * 2, 2, 16)
    header += b"data" + struct.pack("<I", data_size)
    with open(path, "wb") as fh:
        fh.write(header + bytes(frames))


# --------------------------------------------------------------------------- #
def main() -> None:
    shutil.rmtree(ROOT, ignore_errors=True)
    for d in (
        "photos/2024",
        "photos/trip",
        "docs/backup",
        "music",
        ".hidden",
        "catalog",
    ):
        os.makedirs(f"{ROOT}/{d}", exist_ok=True)

    # 1) 精确重复的文本（同名不同目录 + 不同文件名）
    same_text = b"fileclean smoke test payload " * 40  # 约 1 KB
    for rel in ("docs/report.txt", "docs/backup/report-copy.txt", "docs/backup/renamed.txt"):
        with open(f"{ROOT}/{rel}", "wb") as f:
            f.write(same_text)

    # 2) 同体积但内容不同 —— 不应判为重复
    with open(f"{ROOT}/docs/different-a.bin", "wb") as f:
        f.write(b"A" * 1024)
    with open(f"{ROOT}/docs/different-b.bin", "wb") as f:
        f.write(b"B" * 1024)

    # 3) 逐字节相同的图片（走精确重复通道）
    write_scene(f"{ROOT}/photos/2024/beach.png", 256, 256, seed=1)
    shutil.copyfile(f"{ROOT}/photos/2024/beach.png", f"{ROOT}/photos/2024/beach-copy.png")

    # 4) 感知相似：同 seed 不同分辨率 + 调色。刻意**不**做成逐字节副本，
    #    否则会在精确阶段被消耗，感知阶段看不到它们。
    write_scene(f"{ROOT}/photos/trip/sunset.png", 256, 256, seed=7)
    write_scene(f"{ROOT}/photos/trip/sunset-hd.png", 320, 320, seed=7, bright=10.0)

    # 5) 对照组：完全不同的场景，不应被合并
    write_scene(f"{ROOT}/photos/trip/different.png", 256, 256, seed=42)

    # 6) RAW + JPEG 配对（内容故意不同，用于验证不被当成重复）
    with open(f"{ROOT}/photos/2024/IMG_0001.CR2", "wb") as f:
        f.write(b"FAKE-RAW-" + b"\x11" * 4096)
    write_scene(f"{ROOT}/photos/2024/IMG_0001.JPG", 64, 64, seed=99)

    # 7) 压缩包内部重复条目
    with zipfile.ZipFile(f"{ROOT}/docs/bundle.zip", "w", zipfile.ZIP_DEFLATED) as zf:
        zf.writestr("inner/data.txt", "zip entry payload " * 60)
        zf.writestr("inner/copy/data.txt", "zip entry payload " * 60)
        zf.writestr("inner/unique.txt", "only one of me")

    # 8) 隐藏文件（默认跳过）
    with open(f"{ROOT}/.hidden/secret.txt", "wb") as f:
        f.write(same_text)

    # 9) 硬链接（同一 inode 的两个路径，不应算作可回收空间）
    os.link(f"{ROOT}/docs/report.txt", f"{ROOT}/docs/backup/hardlink.txt")

    # 10) 零字节文件（默认 min_size=1 跳过）
    open(f"{ROOT}/docs/empty-a.txt", "wb").close()
    open(f"{ROOT}/docs/empty-b.txt", "wb").close()

    # 11) 伪造 Lightroom 目录（SQLite），引用 docs/report.txt
    conn = sqlite3.connect(f"{ROOT}/catalog/Smoke.lrcat")
    conn.executescript(
        """
        CREATE TABLE AgLibraryRootFolder (id INTEGER PRIMARY KEY, absolutePath TEXT);
        CREATE TABLE AgLibraryFolder (id INTEGER PRIMARY KEY, rootFolder INTEGER, pathFromRoot TEXT);
        CREATE TABLE AgLibraryFile (id INTEGER PRIMARY KEY, folder INTEGER, baseName TEXT, extension TEXT);
        """
    )
    conn.execute(
        "INSERT INTO AgLibraryRootFolder (id, absolutePath) VALUES (1, ?)",
        (os.path.realpath(f"{ROOT}/docs") + "/",),
    )
    conn.execute("INSERT INTO AgLibraryFolder (id, rootFolder, pathFromRoot) VALUES (1, 1, '')")
    conn.execute(
        "INSERT INTO AgLibraryFile (id, folder, baseName, extension) VALUES (1, 1, 'report', 'txt')"
    )
    conn.commit()
    conn.close()

    # 12) 同一段音频的不同采样率 —— 音频指纹应判为相似
    sweep = lambda t: 220.0 + 120.0 * math.sin(2.0 * math.pi * t / 6.0)  # noqa: E731
    write_wav(f"{ROOT}/music/song-a.wav", 12.0, 44100, sweep)
    write_wav(f"{ROOT}/music/song-a-lowrate.wav", 12.0, 22050, sweep)

    # 13) 完全不同的音频 —— 不应命中
    write_wav(f"{ROOT}/music/other.wav", 12.0, 44100, lambda t: 1400.0 + 300.0 * math.sin(t))

    count = sum(len(fns) for _, _, fns in os.walk(ROOT))
    total = sum(
        os.path.getsize(os.path.join(dp, fn))
        for dp, _, fns in os.walk(ROOT)
        for fn in fns
    )
    print(f"fixture ready: {ROOT}  files={count}  bytes={total}")


if __name__ == "__main__":
    main()
