# FileClean

Tauri 2 + Rust 的桌面端重复文件扫描与安全清理工具。**扫描快、判定准、删得稳** —— 所有清理动作都建立在可回退的安全机制之上。

```
┌─────────────────────────────────────────────────────────────┐
│  React 19 + TypeScript  (WebView)                           │
│    ├─ 扫描配置 / 过滤面板 / 实时进度                          │
│    ├─ 虚拟滚动分组列表 + 详情面板                             │
│    └─ 图片对比 / 文本差异 / 音频频谱预览                       │
└──────────────────────────┬──────────────────────────────────┘
                           │  Tauri IPC（类型化 invoke + 事件通道）
┌──────────────────────────┴──────────────────────────────────┐
│  Rust 后端                                                   │
│    commands/   控制器：解析入参 → 调服务 → 格式化响应           │
│    pipeline     服务编排：遍历→预筛→哈希→相似→引用→标记        │
│    scan/        目录遍历、过滤规则、系统目录保护锁              │
│    hash/        MD5/SHA-1/SHA-256/xxHash3/BLAKE3 + mmap        │
│    dedupe/      精确分组 / pHash+dHash / BK-tree / 音频指纹     │
│    archive/     ZIP·7Z 原生条目扫描（RAR 借 7z CLI）           │
│    refs/        Lightroom / Apple Music / Premiere 引用检查    │
│    autoselect/  规则链式保留策略 + 反向选择                     │
│    actions/     回收站 / 硬链接替换 / 隔离区 + 撤销             │
│    export/      CSV / HTML / XML / JSON 报告                   │
│    db/          SQLite 增量缓存、会话、操作审计流水             │
└─────────────────────────────────────────────────────────────┘
```

---

## 一、快速开始

```bash
# 安装前端依赖
npm install

# 开发模式（热重载，同时编译 Rust）
npm run tauri:dev

# 打包发布
npm run tauri:build
```

### CLI 模式

不带子命令时启动图形界面；带子命令时走命令行，可嵌入备份脚本 / NAS 定时任务 / CI。

```bash
# 基础扫描（人类可读摘要走 stderr）
fileclean scan ~/Pictures ~/Downloads

# 生成 HTML 报告
fileclean scan /Volumes/Data --format html --output report.html

# 机器可读输出，交给 jq 处理
fileclean scan ~/Photos --json | jq '.stats.wastedBytes'

# 照片场景：只查图片 + 感知哈希 92% 阈值
fileclean scan ~/Pictures --include-ext jpg,heic,raw --perceptual --perceptual-threshold 0.92

# 深度审计：全部能力打开，导出 JSON 归档
fileclean scan ~/Photos ~/Music --perceptual --audio --archives --references --format json -o audit.json

# 先演练，确认清单无误再真实执行
fileclean scan ~/Downloads --trash --dry-run
fileclean scan ~/Downloads --trash --yes

# 硬链接替换（省空间且保留原路径引用）
fileclean scan /data --hardlink --yes

# 追加自定义保留规则：RAW 优先 → 再按创建时间最早
fileclean scan ~/Pictures --rule prefer-raw --rule oldest-created --move-to /tmp/quarantine --yes

fileclean rules       # 查看内置保留策略
fileclean sessions    # 列出已保存会话
```

**退出码**：`0` 正常、`1` 出错、`3` 发现了重复项（便于 `if fileclean scan ...; then` 分支）。

---

## 二、界面结构

窗口默认 **1200 × 800**（最小 1000 × 660），顶栏是一个两档的视图切换：

| 视图 | 定位 | 内容 |
|---|---|---|
| **扫描** | 工作台，只放"每次扫描都可能要动"的东西 | 扫描目录、4 个快捷场景、**当前规则只读摘要**、开始/取消 |
| **扫描规则** | 设置页，规则在这里集中改 | 6 个分区，左侧导航 + 右侧宽屏表单 |

设置页的六个分区按「规则 → 安全性能 → 策略数据」三段组织：

| 段 | 分区 | 内容 |
|---|---|---|
| 扫描规则 | 过滤条件 | 扩展名白/黑名单、体积范围、修改日期、glob 排除规则、文件属性 |
| | 匹配算法 | 哈希算法（含各自适用场景说明）、RAW/JPEG 配对、图片感知哈希、音频指纹、压缩包内检索 |
| 安全与性能 | 安全与保护 | 系统保护锁、引用完整性检查，以及四条**不随设置变化**的安全红线 |
| | 性能与缓存 | 并发线程、分组门槛、增量缓存（含清空/打开导出目录）、运行环境 |
| 策略与数据 | 智能保留策略 | 规则链编辑器、每组保底份数、受保护文件策略 |
| | 会话与导入导出 | 会话保存 / 载入 / 导入导出文件 |

**为什么把配置拆出去**：原先 6 组配置全挤在 336px 的侧栏里、用折叠面板收纳，窄栏里一行放不下两个字段，`<details>` 展开后还要来回滚动。拆成独立页面后表单能用两列布局、每个字段都能带一行说明，主界面也只剩下真正需要动手的东西。

主界面侧栏保留的那份**只读摘要**是有意为之：它把"会影响本次扫描结果"的规则（哈希算法、过滤条件、分组门槛、并发、缓存、保留策略条数与保底份数）压成一眼能看完的几行，再往下才是"调整扫描规则"的入口。这样改完规则回到主界面，不必凭记忆确认配置。

### 界面语言与主题

支持 **中文 / English** 双语界面，以及 **亮色 / 暗色 / 跟随系统** 三档主题，均在「扫描规则 → 外观」里设置：

- **界面语言**：下拉切换，立即生效、无需重启；偏好存于 `localStorage` 的 `fileclean.lang`。
- **界面主题**：亮色、暗色，或跟随系统（实时匹配操作系统的浅色 / 深色外观）；偏好存于 `fileclean.theme`，默认「跟随系统」。

实现要点：`src/i18n/` 是轻量自建方案——`I18nProvider` + `useI18n()` + `t(key, vars?)`，以 `zh.ts` 为唯一真源，`en.ts` 用 `Record<DictKey, string>` 在编译期强制键完整，纯逻辑模块通过模块级单例直接调用 `t()`。主题由 `window.matchMedia('(prefers-color-scheme: dark)')` 驱动，作用于 `document.documentElement.dataset.theme`，暗色调色板在 `src/styles.css` 的 `[data-theme='dark']` 中覆盖。

各视图在 1200 × 800 下的实际渲染见 `docs/screenshots/`（含设置页六个分区，以及 1000 × 660 最小尺寸的对照）。这些图由 `npm run preview:ui` + 无头浏览器按真实窗口尺寸截取，不是效果图。

---

## 三、核心能力

### 1. 多级匹配算法

| 层级 | 手段 | 作用 |
|---|---|---|
| 预筛 | 文件体积分组 | 体积唯一的文件直接出局，通常是最大的性能收益 |
| 预筛 | 分块指纹（首尾各 64 KiB + 体积 → xxHash3） | 几十毫秒筛掉「体积相同但内容不同」的文件 |
| 判定 | 全量哈希：BLAKE3 / SHA-256 / SHA-1 / MD5 / xxHash3 | **最终判定依据**，文件名不同也能识别 |
| 相似 | 图片 pHash + dHash（DCT 感知哈希） | 识别裁剪、调色、压缩、改尺寸后的同一张照片 |
| 相似 | 音频频谱轮廓指纹（Symphonia 解码 + FFT + 24 对数频带） | 识别不同码率、不同采样率但听感相同的音频 |

相似检索用 **BK-tree**（汉明距离满足三角不等式）做半径查询，并查集收敛传递关系；相似度阈值由界面滑块 / `--perceptual-threshold` 控制。

两个容易被忽略但决定成败的细节：

- **pHash 的比特是基于「低频系数 vs 中位数」判定的**，对整体调色、重新压缩天然免疫（实测同一张照片从 4096×2160 缩到 640×337、JPEG q35 重压、转成 PNG，pHash 距离均为 **0**）。但对**频谱稀疏**的图片（纯色块、单频渐变、矢量图）会退化 —— 详见「已知限制」。
- **音频的 24 条对数频带用固定的绝对频率网格**（40 Hz → 16 kHz），**不随采样率浮动**。若频带边界跟着采样率走，「第 k 条频带」在 44.1 kHz 与 22.05 kHz 文件里对应的实际频率就不同，两条轮廓根本不可比 —— 实测同一首歌的不同采样率版本相似度会掉到 0.92 以下而漏检。改成固定网格后为 **0.98**。

### 2. 灵活的扫描范围与过滤

- 多根目录、网络路径、外接磁盘、云同步目录
- 扩展名白名单 / 黑名单、体积区间、修改时间区间
- 隐藏文件 / 系统文件开关、glob 排除规则、最小目录深度
- 符号链接默认不跟随（避免链接成环导致重复计数）

### 3. 安全删除机制

| 方式 | 行为 | 可撤销 |
|---|---|---|
| **移入回收站** | 默认方式，文件进系统废纸篓 | 从废纸篓手动还原 |
| **移入指定回收目录** | 设 `FILECLEAN_TRASH_DIR` 后启用，完全不碰系统回收站 | 从该目录搬回 |
| **硬链接替换** | 副本位置替换为指向保留文件的硬链接，省空间且**原路径引用不失效**（Lightroom / 剪辑工程不会丢素材） | 不可逆（原路径已指向保留文件） |
| **移入隔离区** | 保留原目录结构移动到隔离目录，供人工二次审核 | **应用内一键撤销还原** |

硬链接替换采用「先建临时链接 → 原子 rename 覆盖」，中途失败不会留下「文件没了但链接没建上」的空窗。

**回收站有降级方案**：macOS 的原生回收站要借道 Finder 自动化，在无 GUI 会话（CI/CD、SSH、定时任务 —— 恰恰是 CLI 的主场）会被 TCC 直接拒绝（`-10004 权限违例`）。此时自动退化为把文件移入用户回收站目录（macOS `~/.Trash`、Linux `~/.local/share/Trash/files`），同名自动改名。代价是失去系统的「放回原处」元数据，但**文件仍可人工找回，绝不会变成永久删除**。要彻底摆脱对运行环境的依赖，用 `FILECLEAN_TRASH_DIR` 把落点钉死。

> 这条降级链有个容易忽略的后果：`$HOME` 只能重定向**降级方案**，重定向不了系统原生回收站。所以「用 `HOME` 沙箱化测试」并不成立 —— 原生路径可用时文件仍会进开发者真实的废纸篓，而 macOS 的 `~/.Trash` 受 TCC 保护，测试进程连列举都会被拒绝，根本无法断言可读回性。这正是 `FILECLEAN_TRASH_DIR` 存在的第二个原因。

### 4. 结果预览与对比

- **图片**：并排缩略图 + 尺寸对比
- **文本**：逐字节定位首个差异偏移，展示差异位置上下文片段
- **音频**：24 频带频谱轮廓柱状图 + 时长
- **二进制**：明确提示内容一致性与一致前缀长度
- 缩略图由 Rust 侧编码为 data URL 内联返回，不开放 asset 协议、不放宽 CSP

### 5. 智能选择助手（Auto-Mark）

规则链按数组顺序构成**字典序优先级**，前一条分出胜负就不再往后看：

```
优先保留位于「原片」的文件
  → 扩展名优先级 CR2 > CR3 > NEF > ARW > DNG
    → 保留最早创建
      → 保留路径最短
```

内置 10 种规则：保留最新/最早修改、最早创建、路径最短、名字最长、层级最浅、体积最大、硬链接最多、指定目录优先、扩展名优先级。支持拖动排序、单条启停、**反向选择**。

**保底机制**：每组至少保留 `keepPerGroup` 份（默认 1），任何情况下都不会清空整组。

### 6. 引用完整性保护

- **Lightroom**：直接查询 `*.lrcat`（SQLite），关联 `AgLibraryFile × AgLibraryFolder × AgLibraryRootFolder` 还原完整路径
- **Apple Music / iTunes**：解析资料库 plist XML 中的 `file://` 引用并百分号解码
- **Premiere**：解压 `*.prproj`（gzip XML），提取 `<ActualMediaFilePath>` 与 `FilePath`
- **链接解析**：符号链接、macOS Alias（`book`/`mark` 魔数）、Windows `.lnk`（启发式提取）
- **系统关键目录保护锁**：`/System`、`/usr`、`/bin`、`/sbin`、`/private/etc`、`/private/var`、`~/Library`、`~/Library/Application Support/MobileSync` 等在遍历阶段就被剪枝，用户即便把 `/` 选为扫描根也不会碰到

  这里刻意**只锁 `/private/etc` 与 `/private/var` 而不是整个 `/private`** —— 因为 macOS 的 `/tmp` 是指向 `/private/tmp` 的软链，锁整个 `/private` 会把临时目录一并误伤，导致扫描静默返回「0 个文件」。另外，**若所有扫描根都被拒绝，会明确报错而不是静默成功**，并提供关闭保护锁的提示。注意系统临时目录 `$TMPDIR`（`/var/folders/... `）也位于保护范围内，如需扫描请用 `--no-protect-system`。

被引用的文件会被标记 `protected`，UI 与动作层**双重拒绝**将其标记为待清理。

### 7. 高性能与增量扫描

- **rayon 多线程** + 每文件 1 MiB 流式读取，I/O 与 CPU 同时吃满
- **内存映射**：≥ 32 MiB 的文件走 mmap；BLAKE3 还启用 `update_mmap_rayon`，单个 ISO / 4K 视频内部也能跑满多核
- **增量缓存**：SQLite 记录 `路径 + 体积 + 修改时间 + inode` 四元组与各级指纹，下次只重算新增/修改的文件，数小时的全量扫描可缩短到几分钟
- 256 文件级进度节流，事件不淹没 WebView

### 8. 特殊格式支持

- **压缩包内扫描**：ZIP 原生流式读取；7Z 原生流式读取；RAR 借系统 `7z` / `7zz` CLI 的列表输出（尺寸 + CRC32）。ZIP 内条目可直接移除（重写压缩包 + 原子替换）
- **RAW + JPEG 配对**：同名 RAW/JPEG 识别为一组，标记为「信息性分组」整体保留，不当作重复；同时把这些文件从精确/相似候选中剔除，避免误报
- **硬链接同源检测**：同一 `(device, inode)` 的多个路径单独标保护 —— 删掉其中一个不会释放任何空间

---

## 四、安全模型

三条不可逾越的红线，写在 `actions::execute` 与 `prepare_target` 里：

1. **无确认不执行** —— 真实执行必须由前端显式传入 `confirmed = true`；CLI 需要 `--yes`
   - `dryRun` 与 `confirmed` 是**两个独立语义**，不要混用：前者是「要不要落盘」，后者是「用户是否已确认」。
     演练（`dryRun = true`）不需要确认，所以只用演练路径测不出「确认标志传错」这类缺陷。
   - 界面上的确认来源是「我已核对以上清单，确认执行」这个勾选项：未勾选时按钮禁用，
     勾选后才以 `confirmed = true` 调用后端。切回演练会重置该勾选。
   - 回归测试：`scripts/smoke_test.py` 场景 9 会断言「未确认的真实执行退出码为 1，且文件绝不被改动」。
2. **不越界** —— 目标必须是绝对路径、必须是普通文件（拒绝符号链接与目录）、必须位于本次扫描根之内、必须不在系统保护目录内
3. **可追溯** —— 每次执行写审计流水（SQLite `action_journal` + `data/journals/<id>.json`），隔离区模式支持一键撤销

补充约束：

- 令牌、路径等敏感信息不进日志（`logging::redact_path` 会把家目录前缀折叠为 `~`）
- 环境变量在启动时集中校验，配置错误**快速失败**并打印原因，而不是留下一个白屏窗口
- 数据库变更通过 `PRAGMA user_version` 管理的可回滚迁移

---

## 五、配置

所有配置来自环境变量，见 `src-tauri/.env.example`。真实 `.env` 已加入 `.gitignore`，**不要提交**。

| 变量 | 默认 | 说明 |
|---|---|---|
| `FILECLEAN_DATA_DIR` | 平台数据目录 | 数据库 / 日志 / 隔离区 / 导出根目录 |
| `FILECLEAN_LOG_LEVEL` | `info` | 日志级别 |
| `FILECLEAN_LOG_JSON` | `false` | 输出 JSON 行协议，便于日志采集 |
| `FILECLEAN_THREADS` | `0`（自动） | 并发线程数 |
| `FILECLEAN_MAX_SCAN_FILES` | `2000000` | 单次扫描文件数硬上限 |
| `FILECLEAN_CACHE_MAX_AGE_DAYS` | `30` | 增量缓存有效期 |
| `FILECLEAN_PROTECT_SYSTEM_DIRS` | `true` | 系统关键目录保护锁 |
| `FILECLEAN_DEFAULT_DELETE_MODE` | `trash` | 默认清理方式 |
| `FILECLEAN_MAX_FULL_HASH_BYTES` | `8589934592` | 超过此体积只做分块指纹 + 抽样 |
| `FILECLEAN_TRASH_DIR` | 未设置 | 显式指定回收目录；**设置后不再调用系统回收站** |

> `FILECLEAN_TRASH_DIR` 是为 CI / 纯 SSH 会话准备的：系统回收站在那些环境里落点不确定
> （macOS 要 Finder 自动化授权、Linux 要 XDG 目录），设置后改为把文件移入指定目录，
> 内容完整保留、随时可搬回。副作用是测试终于能把「回收站」这一步**完全沙箱化**——
> 只设 `HOME` 只能重定向备用方案，管不住系统原生回收站。

---

## 六、开发

```bash
# Rust 单元测试（52 个）
cd src-tauri && cargo test

# 前端类型检查
npm run typecheck

# 端到端冒烟测试（自动建 fixture → 跑 14 个场景 → 79 项断言）
python3 scripts/smoke_test.py

# 只做感知哈希 / 音频指纹的手工诊断
cd src-tauri
cargo run --example perc_probe -- a.png b.png
cargo run --example audio_probe -- a.wav b.wav

# 无后端 UI 预览（不编译 Rust，直接看版式）
npm run preview:ui          # → http://localhost:4175/preview.html
                            #   加 ?view=settings 直接进设置页
                            #   加 ?demo=1 自动载入一份演示扫描结果

# 完整构建
npm run tauri:build
```

`npm run preview:ui` 用官方 `@tauri-apps/api/mocks` 顶替 IPC，返回固定的假数据，让 `vite` 单跑前端。改版式时不必等 Rust 编译，也方便在没有窗口环境的机器上截图验收。假数据是**类型化**的，契约一旦漂移 `tsc` 会在 `src/preview.tsx` 先报错，相当于一道防漂移哨兵。

> **注意 URL 必须是 `/preview.html`。** 根路径 `/` 提供的是生产入口 `index.html`，那里没有 mocks —— 控制台会报 `Cannot read properties of undefined (reading 'transformCallback')`，界面看似能渲染但交互失效。
>
> 同理，`vite.preview.config.ts` 里必须显式写 `build.rollupOptions.input: "preview.html"`。Vite 默认把根目录 `index.html` 当入口，漏掉这一行的话 `vite build --config vite.preview.config.ts` 会打出与正式产物**同名同哈希**的 bundle，看起来构建通过，其实预览入口压根没编译。

`scripts/smoke_test.py` 会在一处临时目录里搭出完整的测试树（精确重复、硬链接、感知相似图、同曲不同采样率、ZIP 内重复、伪造 Lightroom 目录……），然后真实地跑一遍 CLI：扫描、增量缓存、**多扫描根**、四种报告导出、演练、回收站、硬链接替换、隔离移动、系统保护锁。清理类场景都在**一次性副本**上执行，并把子进程的 `HOME` 与 `FILECLEAN_TRASH_DIR` 都指向沙箱，不会碰到你的真实文件或回收站。

`scripts/make_fixture.py` 是 fixture 生成器，纯标准库、无第三方依赖。

测试覆盖了最容易出错的地方：哈希算法一致性、硬链接同源识别、BK-tree 半径查询、采样率无关的音频轮廓、规则链优先级、越界拒绝、演练模式不落盘、**未确认的真实执行被拒绝且不落盘**、隔离区目录结构保留与撤销还原、系统保护锁边界、**多扫描根下的增量缓存加载**。

> 为什么专门测「多扫描根」：`load_for_roots` 的动态 SQL 曾用位置参数手算下标，第二个根会复用 `?3`，导致 `Got 5, needed 4`。**只有一个根时恰好正确**，所以任何只用单根跑的测试都发现不了。这类循环展开的代码必须覆盖 N ≥ 2。

---

## 七、已知限制

诚实列出边界，避免误用：

1. **音频指纹是中段无关的频谱轮廓比对**，不是完整的 chromaprint 峰值对哈希。它能识别「同一首歌的不同压制品」，但**不做变调 / 变速 / 中段裁剪匹配**，且默认阈值下仍建议人工确认。
2. **感知哈希对频谱稀疏的图片会退化**。pHash 的 63 个比特取自 DCT 低频系数与中位数的大小关系；对真实照片（频谱丰富）非常稳，但对纯色块、单频渐变、极简矢量图这类低频系数大部分接近 0 的图片，比特会被数值噪声左右，同一张图换个分辨率也可能产生十几位差异。实测 15 张系统壁纸两两组合（105 对）**零误判**，但对合成图不要期望过高。另外，**已进入精确重复组的文件不再参与相似检测**（避免同一堆文件被报两次），所以互为逐字节副本的图片不会出现在相似分组里。
3. **RAR 需要系统安装 7-Zip 命令行工具**（`7z` / `7zz`），且只用其 CRC32 列表作为弱指纹。缺失工具时会明确报错而不是静默跳过。
4. **`.lnk` 解析是启发式的**（扫描二进制中的路径字符串）。Shell Link 完整格式解析未实现，命中失败只会**漏掉保护**而不会误删。
5. **别名解析依赖 `osascript` 调用 Finder**，在无 GUI 会话（如纯 SSH）下不可用；数量上限 300 条。回收站同理，但有降级方案（见「安全删除机制」）。
6. **mmap 与外部修改存在固有竞态**：若文件在映射期间被外部截断，理论上可能触发 SIGBUS。已把阈值设在 32 MiB 并在 `try_mmap_hash` 失败时回退到流式读取。
7. **硬链接替换要求同文件系统**；跨卷条目会被跳过并如实报错。
8. 感知哈希跳过超过 1.2 亿像素的图片，避免异常图片打爆内存。

---

## 八、目录结构

```
FileClean/
├── index.html
├── preview.html                 无后端 UI 预览入口
├── package.json
├── vite.config.ts
├── vite.preview.config.ts       预览专用配置（端口 4175）
├── tsconfig.json
├── scripts/                    测试工具（纯标准库）
│   ├── make_fixture.py         生成覆盖全部场景的测试目录树
│   └── smoke_test.py           端到端冒烟测试（14 场景 / 79 断言）
├── docs/screenshots/           验收截图（1200×800 与 1000×660）
├── src/                        前端
│   ├── main.tsx  App.tsx  styles.css
│   ├── preview.tsx             预览入口（用官方 mocks 顶替 IPC）
│   ├── api.ts                  IPC 客户端（类型化 invoke + 事件订阅 + 错误映射）
│   ├── types.ts                与 Rust models 一一对应的类型契约
│   ├── format.ts  thumbnails.ts
│   └── components/
│       ├── SetupPanel.tsx      主界面侧栏：扫描目录 / 快捷场景 / 规则只读摘要 / 开始扫描
│       ├── SettingsPage.tsx    扫描规则设置页（左侧分区导航 + 右侧内容区）
│       ├── settings/           设置页六个分区
│       │   ├── FilterSettings.tsx        过滤条件
│       │   ├── AlgorithmSettings.tsx     匹配算法
│       │   ├── SafetySettings.tsx        安全与保护
│       │   ├── PerformanceSettings.tsx   性能与缓存
│       │   └── MarkingSettings.tsx       智能保留策略
│       ├── RuleEditor.tsx      保留策略规则链编辑器
│       ├── GroupList.tsx       虚拟滚动分组列表
│       ├── GroupDetail.tsx     文件列表 + 图片/文本/音频预览
│       ├── ActionDialog.tsx    清理确认（方式选择 + 演练 + 拦截清单 + 显式确认勾选）
│       ├── SessionPanel.tsx    会话保存 / 载入 / 导入导出
│       ├── Toasts.tsx  Icon.tsx
└── src-tauri/                  后端
    ├── Cargo.toml  build.rs  tauri.conf.json
    ├── capabilities/default.json
    ├── .env.example
    ├── examples/               手工诊断小工具
    │   ├── perc_probe.rs       打印 pHash/dHash 与两两汉明距离
    │   └── audio_probe.rs      打印音频轮廓与两两余弦相似度
    └── src/
        ├── main.rs  lib.rs  cli.rs
        ├── config.rs  error.rs  logging.rs  state.rs  pipeline.rs
        ├── commands/   scan_cmd · result_cmd · action_cmd · session_cmd
        ├── domain/     models.rs（前后端共享数据契约）
        ├── db/         mod · migrations · cache_repo · session_repo
        ├── scan/       filter · walker
        ├── hash/       algorithms · engine
        ├── dedupe/     mod(UnionFind) · exact · perceptual · audio · raw_pair
        ├── archive/    mod
        ├── refs/       mod
        ├── autoselect/ mod
        ├── actions/    mod
        └── export/     mod
```
