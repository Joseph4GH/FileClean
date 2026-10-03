#!/usr/bin/env python3
"""FileClean 端到端冒烟测试。

覆盖：精确重复、RAW/JPEG 配对、感知哈希、音频指纹、压缩包内部条目、
引用完整性保护、增量缓存、报告导出、以及三种清理模式（回收站/硬链接替换/隔离移动）。

用法：
    python3 scripts/smoke_test.py            # 自动构建 debug 二进制后运行
    python3 scripts/smoke_test.py --no-build # 复用已有二进制

退出码：0 = 全部通过，1 = 有失败。
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
BIN = REPO / "src-tauri" / "target" / "debug" / "fileclean"
FIXTURE_SRC = REPO / "scripts" / "make_fixture.py"
RUN_TIMEOUT = 120  # 单条命令上限，防止图形界面路径把测试挂死


# --------------------------------------------------------------------------- #
# 测试框架
# --------------------------------------------------------------------------- #
class Runner:
    def __init__(self) -> None:
        self.passed = 0
        self.failed = 0
        self.failures: list[str] = []
        self._scenario = ""

    def scenario(self, name: str) -> None:
        self._scenario = name
        print(f"\n\033[1m=== {name} ===\033[0m")

    def check(self, label: str, ok: bool, detail: str = "") -> None:
        if ok:
            self.passed += 1
            print(f"  \033[32m✓\033[0m {label}" + (f"  ({detail})" if detail else ""))
        else:
            self.failed += 1
            msg = f"[{self._scenario}] {label}" + (f" — {detail}" if detail else "")
            self.failures.append(msg)
            print(f"  \033[31m✗\033[0m {label}" + (f"  ({detail})" if detail else ""))

    def eq(self, label: str, actual, expected) -> None:
        self.check(label, actual == expected, f"期望 {expected!r}，实际 {actual!r}")

    def summary(self) -> int:
        print("\n" + "=" * 62)
        if self.failed == 0:
            print(f"\033[32m全部通过\033[0m：{self.passed} 项断言")
            return 0
        print(f"\033[31m失败 {self.failed} 项\033[0m / 通过 {self.passed} 项")
        for f in self.failures:
            print(f"  · {f}")
        return 1


R = Runner()


# --------------------------------------------------------------------------- #
# 执行辅助
# --------------------------------------------------------------------------- #
def run(
    args: list[str],
    data_dir: Path,
    *,
    expect_codes=(0, 3),
    env_extra: dict[str, str] | None = None,
) -> tuple[int, str, str]:
    env = dict(os.environ)
    env["FILECLEAN_DATA_DIR"] = str(data_dir)
    env.setdefault("FILECLEAN_LOG_LEVEL", "warn")
    if env_extra:
        env.update({k: str(v) for k, v in env_extra.items()})
    try:
        proc = subprocess.run(
            [str(BIN), *args],
            capture_output=True,
            text=True,
            env=env,
            stdin=subprocess.DEVNULL,
            timeout=RUN_TIMEOUT,
        )
    except subprocess.TimeoutExpired:
        # 必须显式兜底：不带子命令时二进制会启动图形界面并永久阻塞，
        # 与其把整轮测试拖死，不如立刻报错。
        R.check(f"命令在 {RUN_TIMEOUT}s 内未返回", False, " ".join(args))
        return 124, "", "timeout"
    if proc.returncode not in expect_codes:
        print(f"  ! 命令异常退出 {proc.returncode}: {' '.join(args)}")
        print("  " + proc.stderr.strip()[:800])
    return proc.returncode, proc.stdout, proc.stderr


def run_scan(
    args: list[str],
    data_dir: Path,
    *,
    expect_codes=(0, 3),
    env_extra: dict[str, str] | None = None,
) -> tuple[int, str, str]:
    """执行 `fileclean scan ...`。

    必须显式带上 `scan` 子命令 —— 不传子命令时二进制会走 `run_gui()`
    启动图形界面，在无头环境下会一直挂住（这个坑真的踩过）。
    """
    return run(["scan", *args], data_dir, expect_codes=expect_codes, env_extra=env_extra)


def scan_json(args: list[str], data_dir: Path) -> dict:
    _, out, _ = run_scan([*args, "--json", "-q"], data_dir)
    return json.loads(out)


def group_by_kind(result: dict) -> dict[str, list[dict]]:
    out: dict[str, list[dict]] = {}
    for g in result["groups"]:
        out.setdefault(g["kind"], []).append(g)
    return out


def rels(group: dict) -> set[str]:
    return {f["entry"]["relativePath"] for f in group["files"]}


def make_fixture(dest: Path) -> None:
    """调用生成脚本造出模板目录（只做一次）。"""
    subprocess.run(
        [sys.executable, str(FIXTURE_SRC), str(dest)],
        check=True,
        capture_output=True,
    )


TEMPLATE: Path | None = None


def fresh_fixture(data: Path, name: str) -> Path:
    """为单个场景复制一份干净的 fixture。

    走自己实现的复制而不是 `shutil.copytree`：后者会把硬链接展开成独立文件，
    导致「硬链接同源保护」相关的断言全部失真。
    """
    assert TEMPLATE is not None, "模板尚未生成"
    dest = data / name
    shutil.rmtree(dest, ignore_errors=True)
    dest.mkdir(parents=True, exist_ok=True)

    seen: dict[tuple[int, int], Path] = {}
    for src in sorted(TEMPLATE.rglob("*")):
        rel = src.relative_to(TEMPLATE)
        dst = dest / rel
        if src.is_dir():
            dst.mkdir(parents=True, exist_ok=True)
            continue
        st = src.stat()
        key = (st.st_dev, st.st_ino)
        if key in seen:
            os.link(seen[key], dst)  # 还原硬链接，保住同一 inode
        else:
            shutil.copy2(src, dst)
            seen[key] = dst
    return dest


def inode_of(p: Path) -> int:
    return os.stat(p).st_ino


def work_base() -> Path:
    """选一个不受「系统关键目录保护锁」影响的临时根目录。

    注意：这里**不能**直接用 `tempfile.mkdtemp()` 的默认位置 —— macOS 的 `$TMPDIR`
    位于 `/var/folders/...`，而 `/private/var` 在应用的受保护列表里，
    扫描根会被（正确地）拒绝，导致全部场景假失败。
    """
    override = os.environ.get("FILECLEAN_SMOKE_DIR")
    if override and Path(override).is_dir():
        return Path(override)
    if Path("/tmp").is_dir():
        return Path("/tmp")
    return Path(tempfile.gettempdir())


# --------------------------------------------------------------------------- #
# 场景
# --------------------------------------------------------------------------- #
def s1_default_scan(data: Path) -> None:
    R.scenario("1. 默认扫描（精确重复 / 配对 / 保护）")
    fx = fresh_fixture(data, "fx-default")

    r = scan_json([str(fx), "--no-cache"], data)
    kinds = group_by_kind(r)

    R.eq("分组总数", r["stats"]["groups"], 3)
    R.check("存在 exact 分组", len(kinds.get("exact", [])) == 2, f"{len(kinds.get('exact', []))} 组")
    R.check("存在 rawJpegPair 分组", "rawJpegPair" in kinds)
    R.check("零字节文件被跳过", r["stats"]["filesSkipped"] == 2, f"{r['stats']['filesSkipped']} 个")
    R.check("隐藏文件未被计入", r["stats"]["filesSeen"] < 30, f"filesSeen={r['stats']['filesSeen']}")

    # 硬链接同源 + 配对成员都应受保护
    R.check("受保护文件数 >= 4", r["stats"]["protectedFiles"] >= 4, f"{r['stats']['protectedFiles']} 个")
    R.check("可回收空间 > 0", r["stats"]["wastedBytes"] > 0, f"{r['stats']['wastedBytes']} B")

    for g in kinds.get("exact", []):
        if "photos/2024/beach.png" in rels(g):
            R.check("beach 精确重复组含副本", "photos/2024/beach-copy.png" in rels(g))
        if "docs/report.txt" in rels(g):
            R.check("report 组为 4 项", len(g["files"]) == 4, f"{len(g['files'])} 项")
            R.check("report 组保留两份（硬链接同源）",
                    sum(1 for f in g["files"] if f["decision"] == "keep") == 2)

    # 内容不同、体积相同的文件不应判为重复
    all_paths = {f["entry"]["relativePath"] for g in r["groups"] for f in g["files"]}
    R.check("同体积不同内容未被判重",
            "docs/different-a.bin" not in all_paths and "docs/different-b.bin" not in all_paths)


def s2_incremental(data: Path) -> None:
    R.scenario("2. 增量缓存（第二次应全部命中）")
    fx = fresh_fixture(data, "fx-cache")

    r1 = scan_json([str(fx)], data)
    R.check("首轮无缓存命中", r1["stats"]["cacheHits"] == 0, f"{r1['stats']['cacheHits']} 个")

    r2 = scan_json([str(fx)], data)
    R.check("次轮全部命中", r2["stats"]["cacheHits"] > 0 and r2["stats"]["cacheMisses"] == 0,
            f"命中 {r2['stats']['cacheHits']} / 未命中 {r2['stats']['cacheMisses']}")
    R.eq("两轮分组数一致", r2["stats"]["groups"], r1["stats"]["groups"])
    R.check("次轮更快或相当", r2["stats"]["durationMs"] <= r1["stats"]["durationMs"] + 5,
            f"{r1['stats']['durationMs']}ms -> {r2['stats']['durationMs']}ms")

    # 修改一个文件后，该文件应重新哈希
    target = fx / "docs" / "report.txt"
    target.write_bytes(b"changed payload that is long enough to matter " * 40)
    r3 = scan_json([str(fx)], data)
    R.check("内容变更后缓存失效", r3["stats"]["cacheMisses"] >= 1, f"{r3['stats']['cacheMisses']} 个未命中")


def s2b_multi_root(data: Path) -> None:
    """多扫描根 —— 回归测试：增量缓存加载曾抛 `Got 5, needed 4`。

    动态拼 SQL 时第二个根复用了 `?3`，SQL 只声明 4 个占位符却绑定 5 个值。
    单根不会触发，所以必须专门用「两个根」跑一遍。
    """
    R.scenario("2b. 多扫描根（缓存加载的回归点）")
    a = fresh_fixture(data, "fx-root-a")
    b = fresh_fixture(data, "fx-root-b")

    # 首次就**开启缓存**：出错的那条 SQL 正是在缓存加载时执行的。
    r1 = scan_json([str(a), str(b)], data)
    R.check("双根扫描未报数据库错误", r1["stats"]["filesScanned"] > 0,
            f"参与哈希 {r1['stats']['filesScanned']}")
    R.check("产生了跨根重复分组", r1["stats"]["groups"] > 0, f"{r1['stats']['groups']} 组")

    # 用 entry.root 判断跨根：entry.path 会被规范化（macOS 上 /tmp -> /private/tmp），
    # 直接和扫描根做字符串前缀比较会恒为 False。
    cross = [
        g for g in r1["groups"]
        if len({f["entry"]["root"] for f in g["files"] if f["entry"]["root"]}) > 1
    ]
    R.check("存在同时落在两个根下的分组", len(cross) > 0, f"{len(cross)} 组")

    # 第二次：命中缓存，这条路径才是真正抛 `Got 5, needed 4` 的地方
    r2 = scan_json([str(a), str(b)], data)
    R.check("带缓存的双根扫描命中缓存", r2["stats"]["cacheHits"] > 0,
            f"命中 {r2['stats']['cacheHits']}")
    R.eq("两轮分组数一致", r2["stats"]["groups"], r1["stats"]["groups"])

    # 三个根也要能跑（占位符数量继续增长）
    c = fresh_fixture(data, "fx-root-c")
    r3 = scan_json([str(a), str(b), str(c)], data)
    R.check("三根扫描未报错", r3["stats"]["groups"] > 0, f"{r3['stats']['groups']} 组")
    R.check("三根扫描命中前两棵树的缓存", r3["stats"]["cacheHits"] > 0,
            f"命中 {r3['stats']['cacheHits']}")


def s3_perceptual(data: Path) -> None:
    R.scenario("3. 感知哈希（同图改分辨率 + 调色）")
    fx = fresh_fixture(data, "fx-perc")

    r = scan_json([str(fx), "--perceptual", "--no-cache"], data)
    kinds = group_by_kind(r)
    perc = kinds.get("perceptual", [])

    R.eq("感知分组数", len(perc), 1)
    if perc:
        g = perc[0]
        R.check("包含 sunset 与其高清版",
                rels(g) == {"photos/trip/sunset.png", "photos/trip/sunset-hd.png"}, str(rels(g)))
        R.check("相似度 >= 0.9", (g.get("similarity") or 0) >= 0.9, f"{g.get('similarity')}")

    all_paths = {f["entry"]["relativePath"] for gg in r["groups"] for f in gg["files"]}
    R.check("无关图片未被合并", "photos/trip/different.png" not in all_paths)

    # 提高阈值只能收紧、不能新增分组。
    # 注意 sunset 两张图的 pHash 完全相同（距离 0），任何阈值下都应当仍然成组 ——
    # 这正是「100% 相似」的应有之义，所以这里验证的是单调性而非「变空」。
    strict = scan_json([str(fx), "--perceptual", "--perceptual-threshold", "0.999", "--no-cache"], data)
    strict_sets = [rels(g) for g in group_by_kind(strict).get("perceptual", [])]
    base_sets = [rels(g) for g in perc]
    R.check("提高阈值不会新增分组", all(s in base_sets for s in strict_sets), str(strict_sets))
    all_strict = {f["entry"]["relativePath"] for gg in strict["groups"] for f in gg["files"]}
    R.check("高阈值下无关图片仍未被合并", "photos/trip/different.png" not in all_strict)


def s4_audio(data: Path) -> None:
    R.scenario("4. 音频指纹（同曲不同采样率）")
    fx = fresh_fixture(data, "fx-audio")

    r = scan_json([str(fx), "--audio", "--no-cache"], data)
    groups = group_by_kind(r).get("audio", [])

    R.eq("音频分组数", len(groups), 1)
    if groups:
        g = groups[0]
        R.check("识别出同曲不同采样率",
                rels(g) == {"music/song-a.wav", "music/song-a-lowrate.wav"}, str(rels(g)))
        sim = g.get("similarity") or 0
        R.check("相似度 >= 0.95（采样率无关）", sim >= 0.95, f"{sim:.4f}")

    all_paths = {f["entry"]["relativePath"] for gg in r["groups"] for f in gg["files"]}
    R.check("不同曲目未被合并", "music/other.wav" not in all_paths)

    # 阈值调高到 0.999 应把这对分开
    strict = scan_json([str(fx), "--audio", "--audio-threshold", "0.999", "--no-cache"], data)
    R.check("阈值 0.999 下不再分组",
            len(group_by_kind(strict).get("audio", [])) == 0)


def s5_archives(data: Path) -> None:
    R.scenario("5. 压缩包内部条目")
    fx = fresh_fixture(data, "fx-arch")

    r = scan_json([str(fx), "--archives", "--no-cache"], data)
    groups = group_by_kind(r).get("archiveInternal", [])

    R.eq("压缩包内分组数", len(groups), 1)
    R.check("统计到 3 个条目", r["stats"]["archiveEntries"] == 3, f"{r['stats']['archiveEntries']} 个")
    if groups:
        inners = {f["archiveInnerPath"] for f in groups[0]["files"]}
        R.check("定位到内部路径",
                "inner/data.txt" in inners and "inner/copy/data.txt" in inners, str(inners))
        R.check("唯一条目未被判重", "inner/unique.txt" not in inners)


def s6_references(data: Path) -> None:
    R.scenario("6. 引用完整性（Lightroom 目录）")
    fx = fresh_fixture(data, "fx-refs")

    r = scan_json([str(fx), "--references", "--no-cache"], data)
    R.eq("引用文件数", r["stats"]["referencedFiles"], 1)

    for g in r["groups"]:
        for f in g["files"]:
            if f["entry"]["relativePath"] == "docs/report.txt":
                R.check("被引用文件受保护", f["protected"] is not None, str(f.get("protected")))
                R.eq("被引用文件判定为保留", f["decision"], "keep")

    # 关闭引用检查后，保护应消失
    off = scan_json([str(fx), "--no-cache"], data)
    R.eq("未启用引用检查时 referencedFiles 为 0", off["stats"]["referencedFiles"], 0)


def s7_export(data: Path) -> None:
    R.scenario("7. 报告导出（CSV / HTML / XML / JSON）")
    fx = fresh_fixture(data, "fx-export")

    for fmt in ("csv", "html", "xml", "json"):
        out = data / f"report.{fmt}"
        code, _, _ = run_scan([str(fx), "--no-cache", "-q", "--format", fmt, "-o", str(out)], data)
        size = out.stat().st_size if out.exists() else 0
        R.check(f"{fmt.upper()} 报告已生成且非空", out.exists() and size > 200, f"{size} B")
        if fmt == "csv" and out.exists():
            lines = out.read_text(encoding="utf-8").strip().splitlines()
            R.check("CSV 含表头与数据行", len(lines) >= 2, f"{len(lines)} 行")
        if fmt == "xml" and out.exists():
            R.check("XML 含根节点", out.read_text(encoding="utf-8").lstrip().startswith("<?xml"))
        if fmt == "json" and out.exists():
            json.loads(out.read_text(encoding="utf-8"))
            R.check("JSON 可解析", True)


def s8_dry_run(data: Path) -> None:
    R.scenario("8. 演练模式不落盘")
    fx = fresh_fixture(data, "fx-dry")
    before = sorted(p.relative_to(fx) for p in fx.rglob("*") if p.is_file())

    code, _, err = run_scan([str(fx), "--no-cache", "-q", "--trash", "--yes", "--dry-run"], data)
    after = sorted(p.relative_to(fx) for p in fx.rglob("*") if p.is_file())
    R.check("文件树未发生变化", before == after, f"{len(before)} -> {len(after)}")
    R.eq("退出码表示发现重复", code, 3)

    # 缺少 --yes 时必须拒绝执行
    code2, _, err2 = run_scan([str(fx), "--no-cache", "-q", "--trash"], data, expect_codes=(1, 3))
    after2 = sorted(p.relative_to(fx) for p in fx.rglob("*") if p.is_file())
    R.check("缺少确认时不执行清理", before == after2)
    R.check("拒绝时有明确报错", "确认" in err2 or code2 == 1, err2.strip().splitlines()[-1] if err2.strip() else "")


def s9_trash(data: Path) -> None:
    R.scenario("9. 清理：移入回收站")
    fx = fresh_fixture(data, "fx-trash")
    # 回收站必须完全沙箱化，否则每跑一次测试就往用户真实废纸篓里丢几个文件。
    # 注意：只设 HOME 是不够的 —— 那只重定向「备用方案」，管不住系统原生回收站
    # （macOS 的 ~/.Trash 还受 TCC 保护，本进程根本列举不了）。
    # 所以这里显式指定 FILECLEAN_TRASH_DIR，让落点确定且可断言。
    fake_home = data / "home-trash"
    fake_home.mkdir(parents=True, exist_ok=True)
    trash_dir = data / "trash-bin"

    doomed = fx / "docs" / "backup" / "report-copy.txt"
    R.check("目标文件清理前存在", doomed.exists())

    # --- 确认闸门：真实执行没有显式确认必须被拒绝，且不能碰任何文件 ---
    # 这条对应「违反安全策略：缺少显式确认：真实执行必须由前端传入 confirmed = true」。
    # GUI 的 ActionDialog 曾经把 dryRun 当 confirmed 传给后端，导致真实执行永远失败；
    # 界面点不了，只能用 CLI 把契约钉住。
    code, _, gerr = run_scan(
        [str(fx), "--no-cache", "-q", "--trash"],
        data,
        expect_codes=(0, 1, 3),
    )
    R.eq("未确认的真实执行被拒绝（退出码 1）", code, 1)
    R.check("给出的是显式确认相关提示", "确认" in gerr, gerr.strip()[:120])
    R.check("被拒绝时绝不落盘", doomed.exists(), "文件不该被动")

    code, _, err = run_scan(
        [str(fx), "--no-cache", "-q", "--trash", "--yes"],
        data,
        env_extra={"HOME": str(fake_home), "FILECLEAN_TRASH_DIR": str(trash_dir)},
    )

    R.check("副本已被移除", not doomed.exists(), str(doomed))
    R.check("原始文件仍在", (fx / "docs" / "report.txt").exists())
    R.check(
        "硬链接与配对成员未被误删",
        (fx / "docs" / "backup" / "hardlink.txt").exists()
        and (fx / "photos" / "2024" / "IMG_0001.CR2").exists(),
    )
    R.check("未报告任何失败项", "✗" not in err, err.strip().splitlines()[-1] if err.strip() else "")

    # 被清理的内容必须仍然可找回，且逐字节一致 —— 这是「默认不永久删除」的底线。
    recoverable = sorted(trash_dir.rglob("report-copy.txt"))
    R.check("内容可在回收目录中找回", len(recoverable) == 1, str(recoverable))
    if len(recoverable) == 1:
        R.check(
            "找回的内容与原件逐字节一致",
            recoverable[0].read_bytes() == (fx / "docs" / "report.txt").read_bytes(),
        )


def s10_hardlink(data: Path) -> None:
    R.scenario("10. 清理：硬链接替换（省空间且保留路径）")
    fx = fresh_fixture(data, "fx-hardlink")

    keeper = fx / "docs" / "report.txt"
    victim = fx / "docs" / "backup" / "report-copy.txt"
    before_inode = inode_of(victim)

    code, _, err = run_scan([str(fx), "--no-cache", "-q", "--hardlink", "--yes"], data)

    R.check("副本路径依然存在（换成链接）", victim.exists())
    ok = victim.exists() and inode_of(victim) == inode_of(keeper)
    R.check("副本已与保留文件共享同一 inode", ok,
            f"{inode_of(victim) if victim.exists() else '-'} vs {inode_of(keeper)}")
    R.check("inode 确实发生了变化", inode_of(victim) != before_inode if victim.exists() else False)
    R.eq("链接数 >= 2", os.stat(keeper).st_nlink >= 2, True)


def s11_move_to(data: Path) -> None:
    R.scenario("11. 清理：隔离到指定目录（保留相对结构）")
    fx = fresh_fixture(data, "fx-move")
    quarantine = data / "quarantine"

    victim_rel = "docs/backup/report-copy.txt"
    R.check("目标文件清理前存在", (fx / victim_rel).exists())

    code, _, err = run_scan([str(fx), "--no-cache", "-q", "--move-to", str(quarantine), "--yes"], data)

    R.check("原位置已不存在", not (fx / victim_rel).exists())
    moved = list(quarantine.rglob("report-copy.txt"))
    R.check("文件出现在隔离目录", len(moved) == 1, str(moved))
    if moved:
        R.check("保留了相对目录结构",
                moved[0].relative_to(quarantine).as_posix().endswith(victim_rel),
                moved[0].relative_to(quarantine).as_posix())
        R.check("内容完整", moved[0].read_bytes() == (b"fileclean smoke test payload " * 40))


def s12_system_protection(data: Path) -> None:
    R.scenario("12. 系统关键目录保护锁")
    # 系统临时目录位于 /private/var 下，属于受保护范围。用它的一个空子目录做探针，
    # 避免为了测试去真的遍历整个 $TMPDIR。
    probe = Path(tempfile.mkdtemp(dir=tempfile.gettempdir()))

    code, out, err = run_scan([str(probe), "--no-cache", "-q"], data, expect_codes=(0, 1, 3))
    R.eq("扫描系统保护目录被拒绝（退出码 1）", code, 1)
    R.check("给出了可操作的提示", "保护" in err, err.strip()[:120])

    # 显式关闭保护锁后应当放行
    code2, _, err2 = run_scan(
        [str(probe), "--no-cache", "-q", "--no-protect-system"],
        data,
        expect_codes=(0, 1, 3),
    )
    R.eq("关闭保护锁后可扫描（退出码 0）", code2, 0)

    shutil.rmtree(probe, ignore_errors=True)


def s13_cli_surface(data: Path) -> None:
    R.scenario("13. CLI 辅助命令")
    code, out, _ = run(["rules"], data)
    R.eq("rules 退出码为 0", code, 0)
    R.check(
        "rules 列出了保留策略",
        "原片" in out or "扩展名" in out or "newest" in out,
        out.strip().splitlines()[0][:60] if out.strip() else "",
    )

    code, out, _ = run(["sessions"], data)
    R.eq("sessions 退出码为 0", code, 0)

    code, out, _ = run(["--version"], data)
    R.eq("--version 退出码为 0", code, 0)


# --------------------------------------------------------------------------- #
def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--no-build", action="store_true", help="跳过 cargo build")
    ap.add_argument("--keep", action="store_true", help="保留临时数据目录")
    args = ap.parse_args()

    if not args.no_build:
        print("构建 debug 二进制 …")
        subprocess.run(
            ["cargo", "build", "--bin", "fileclean"],
            cwd=REPO / "src-tauri",
            check=True,
        )
    if not BIN.exists():
        print(f"找不到二进制：{BIN}")
        return 1

    workdir = Path(tempfile.mkdtemp(prefix="fileclean-smoke-", dir=work_base()))
    data = workdir / "data"
    data.mkdir(parents=True, exist_ok=True)
    print(f"临时工作目录：{workdir}")

    global TEMPLATE
    TEMPLATE = workdir / "template"
    print("生成 fixture 模板 …")
    make_fixture(TEMPLATE)

    try:
        for fn in (
            s1_default_scan,
            s2_incremental,
            s2b_multi_root,
            s3_perceptual,
            s4_audio,
            s5_archives,
            s6_references,
            s7_export,
            s8_dry_run,
            s9_trash,
            s10_hardlink,
            s11_move_to,
            s12_system_protection,
            s13_cli_surface,
        ):
            try:
                fn(data)
            except Exception as exc:  # 单个场景异常不应中断整轮测试
                R.failed += 1
                R.failures.append(f"[{fn.__name__}] 抛出异常: {exc!r}")
                print(f"  \033[31m✗\033[0m 场景异常：{exc!r}")
    finally:
        if args.keep:
            print(f"已保留：{workdir}")
        else:
            shutil.rmtree(workdir, ignore_errors=True)

    return R.summary()


if __name__ == "__main__":
    sys.exit(main())
