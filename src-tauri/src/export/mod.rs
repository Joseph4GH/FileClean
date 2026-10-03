//! 报告导出：CSV / HTML / XML / JSON，用于审计、合规归档与二次处理。

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::domain::models::{ExportFormat, MarkDecision, ScanResult};
use crate::error::AppResult;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSummary {
    pub path: String,
    pub format: ExportFormat,
    pub rows: u64,
    pub bytes: u64,
}

pub fn export(result: &ScanResult, format: ExportFormat, out_path: &Path) -> AppResult<ExportSummary> {
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut file = std::fs::File::create(out_path)?;

    match format {
        ExportFormat::Csv => write_csv(result, &mut file)?,
        ExportFormat::Html => write_html(result, &mut file)?,
        ExportFormat::Xml => write_xml(result, &mut file)?,
        ExportFormat::Json => write_json(result, &mut file)?,
    }

    file.flush()?;
    let bytes = std::fs::metadata(out_path).map(|m| m.len()).unwrap_or(0);

    Ok(ExportSummary {
        path: out_path.to_string_lossy().to_string(),
        format,
        rows: count_rows(result),
        bytes,
    })
}

pub fn default_path(_result: &ScanResult, format: ExportFormat, dir: &Path) -> PathBuf {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    dir.join(format!(
        "fileclean-report-{stamp}.{}",
        format.extension()
    ))
}

fn count_rows(result: &ScanResult) -> u64 {
    result
        .groups
        .iter()
        .flat_map(|g| g.files.iter())
        .filter(|f| f.decision != MarkDecision::Keep)
        .count() as u64
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

// ============================ CSV ============================

fn write_csv(result: &ScanResult, out: &mut impl Write) -> AppResult<()> {
    let mut writer = csv::Writer::from_writer(out);

    writer.write_record([
        "组ID",
        "匹配类型",
        "组签名",
        "相似度",
        "决策",
        "是否首选",
        "保护原因",
        "文件名",
        "扩展名",
        "体积(字节)",
        "体积",
        "修改时间",
        "创建时间",
        "目录层级",
        "完整路径",
        "相对路径",
        "判定理由",
    ])?;

    for group in &result.groups {
        let similarity = group
            .similarity
            .map(|s| format!("{:.4}", s))
            .unwrap_or_default();

        for file in &group.files {
            writer.write_record([
                group.id.as_str(),
                group.kind_label.as_str(),
                group.signature.as_str(),
                similarity.as_str(),
                decision_label(file.decision),
                if file.is_preferred { "是" } else { "否" },
                file.protected.as_deref().unwrap_or(""),
                file.entry.file_name(),
                file.entry.extension.as_str(),
                &file.entry.size.to_string(),
                &human_bytes(file.entry.size),
                &format_time(file.entry.modified),
                &format_time(file.entry.created),
                &file.entry.depth.to_string(),
                file.entry.path.as_str(),
                file.entry.relative_path.as_str(),
                &file.reasons.join("；"),
            ])?;
        }
    }

    writer.flush()?;
    Ok(())
}

fn decision_label(d: MarkDecision) -> &'static str {
    match d {
        MarkDecision::Keep => "保留",
        MarkDecision::Remove => "待清理",
        MarkDecision::Undecided => "未决策",
    }
}

fn format_time(unix: i64) -> String {
    if unix <= 0 {
        return String::new();
    }
    chrono::DateTime::from_timestamp(unix, 0)
        .map(|dt| dt.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_default()
}

// ============================ JSON ============================

fn write_json(result: &ScanResult, out: &mut impl Write) -> AppResult<()> {
    serde_json::to_writer_pretty(out, result)?;
    Ok(())
}

// ============================ XML ============================

fn write_xml(result: &ScanResult, out: &mut impl Write) -> AppResult<()> {
    writeln!(out, r#"<?xml version="1.0" encoding="UTF-8"?>"#)?;
    writeln!(
        out,
        r#"<FileCleanReport scanId="{}" algorithm="{}" createdAt="{}">"#,
        esc(&result.scan_id),
        esc(result.algorithm.as_str()),
        esc(&format_time(result.created_at))
    )?;

    writeln!(out, "  <Summary>")?;
    writeln!(out, "    <FilesSeen>{}</FilesSeen>", result.stats.files_seen)?;
    writeln!(out, "    <FilesScanned>{}</FilesScanned>", result.stats.files_scanned)?;
    writeln!(out, "    <Groups>{}</Groups>", result.stats.groups)?;
    writeln!(out, "    <DuplicateFiles>{}</DuplicateFiles>", result.stats.duplicate_files)?;
    writeln!(out, "    <WastedBytes>{}</WastedBytes>", result.stats.wasted_bytes)?;
    writeln!(out, "    <WastedHuman>{}</WastedHuman>", esc(&human_bytes(result.stats.wasted_bytes)))?;
    writeln!(out, "    <DurationMs>{}</DurationMs>", result.stats.duration_ms)?;
    writeln!(out, "    <CacheHits>{}</CacheHits>", result.stats.cache_hits)?;
    writeln!(out, "  </Summary>")?;

    writeln!(out, "  <Roots>")?;
    for root in &result.roots {
        writeln!(out, "    <Root>{}</Root>", esc(root))?;
    }
    writeln!(out, "  </Roots>")?;

    writeln!(out, "  <Groups>")?;
    for group in &result.groups {
        writeln!(
            out,
            r#"    <Group id="{}" kind="{}" signature="{}" wastedBytes="{}" totalBytes="{}"{}>"#,
            esc(&group.id),
            esc(&group.kind_label),
            esc(&group.signature),
            group.wasted_bytes,
            group.total_bytes,
            group
                .similarity
                .map(|s| format!(r#" similarity="{s:.4}""#))
                .unwrap_or_default()
        )?;
        for file in &group.files {
            writeln!(
                out,
                r#"      <File decision="{}" preferred="{}" size="{}" modified="{}" extension="{}" depth="{}">"#,
                decision_label(file.decision),
                file.is_preferred,
                file.entry.size,
                esc(&format_time(file.entry.modified)),
                esc(&file.entry.extension),
                file.entry.depth
            )?;
            writeln!(out, "        <Path>{}</Path>", esc(&file.entry.path))?;
            writeln!(out, "        <Name>{}</Name>", esc(file.entry.file_name()))?;
            if let Some(hash) = &file.hash {
                writeln!(out, "        <Hash>{}</Hash>", esc(hash))?;
            }
            if let Some(protected) = &file.protected {
                writeln!(out, "        <Protected>{}</Protected>", esc(protected))?;
            }
            for reason in &file.reasons {
                writeln!(out, "        <Reason>{}</Reason>", esc(reason))?;
            }
            writeln!(out, "      </File>")?;
        }
        writeln!(out, "    </Group>")?;
    }
    writeln!(out, "  </Groups>")?;
    writeln!(out, "</FileCleanReport>")?;
    Ok(())
}

fn esc(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // XML 1.0 不允许的控制字符：丢弃而不是产出损坏文档。
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => {}
            c => out.push(c),
        }
    }
    out
}

// ============================ HTML ============================

fn write_html(result: &ScanResult, out: &mut impl Write) -> AppResult<()> {
    let waste = human_bytes(result.stats.wasted_bytes);
    let total_waste = result.stats.wasted_bytes;

    write!(
        out,
        r#"<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>FileClean 重复文件报告</title>
<style>
  :root {{
    --bg: #f6f7f9; --card: #ffffff; --line: #e5e7eb; --text: #1f2328;
    --muted: #656d76; --keep: #1a7f37; --remove: #cf222e; --accent: #0969da;
  }}
  * {{ box-sizing: border-box; }}
  body {{ margin: 0; padding: 32px; background: var(--bg); color: var(--text);
         font: 14px/1.6 -apple-system, "PingFang SC", "Microsoft YaHei", system-ui, sans-serif; }}
  h1 {{ font-size: 22px; margin: 0 0 4px; }}
  .sub {{ color: var(--muted); margin-bottom: 24px; }}
  .cards {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: 12px; margin-bottom: 24px; }}
  .card {{ background: var(--card); border: 1px solid var(--line); border-radius: 10px; padding: 16px; }}
  .card .k {{ color: var(--muted); font-size: 12px; }}
  .card .v {{ font-size: 22px; font-weight: 600; margin-top: 4px; }}
  .card .v.em {{ color: var(--remove); }}
  .toolbar {{ display: flex; gap: 8px; margin-bottom: 12px; flex-wrap: wrap; }}
  .toolbar input, .toolbar select {{ padding: 6px 10px; border: 1px solid var(--line);
        border-radius: 6px; background: var(--card); color: var(--text); }}
  table {{ width: 100%; border-collapse: collapse; background: var(--card);
           border: 1px solid var(--line); border-radius: 10px; overflow: hidden; }}
  th, td {{ padding: 8px 10px; border-bottom: 1px solid var(--line); text-align: left; vertical-align: top; }}
  th {{ background: #f0f2f5; font-weight: 600; cursor: pointer; user-select: none; white-space: nowrap; font-size: 12px; }}
  tr:last-child td {{ border-bottom: none; }}
  tr.keep td {{ background: #f0fbf3; }}
  tr.remove td {{ background: #fff5f5; }}
  tr.group-head td {{ background: #eef4ff; font-weight: 600; }}
  .tag {{ display: inline-block; padding: 1px 7px; border-radius: 999px; font-size: 12px; border: 1px solid var(--line); }}
  .tag.keep {{ color: var(--keep); border-color: #b7e0c3; background: #eafaf0; }}
  .tag.remove {{ color: var(--remove); border-color: #f5bcbc; background: #fdecec; }}
  .path {{ font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 12px; word-break: break-all; }}
  .muted {{ color: var(--muted); font-size: 12px; }}
  .reasons {{ color: var(--muted); font-size: 12px; }}
  footer {{ margin-top: 24px; color: var(--muted); font-size: 12px; }}
</style>
</head>
<body>
<h1>FileClean 重复文件报告</h1>
<div class="sub">扫描时间 {created} · 指纹算法 {algo} · 共 {group_count} 个分组</div>

<div class="cards">
  <div class="card"><div class="k">可回收空间</div><div class="v em">{waste}</div></div>
  <div class="card"><div class="k">待清理文件</div><div class="v">{dupes}</div></div>
  <div class="card"><div class="k">扫描文件总数</div><div class="v">{seen}</div></div>
  <div class="card"><div class="k">参与哈希文件</div><div class="v">{scanned}</div></div>
  <div class="card"><div class="k">增量缓存命中</div><div class="v">{cache}</div></div>
  <div class="card"><div class="k">耗时</div><div class="v">{duration}</div></div>
</div>

<div class="toolbar">
  <input id="q" placeholder="按路径 / 文件名过滤…" style="flex:1;min-width:220px">
  <select id="kind">
    <option value="">全部匹配类型</option>
    <option value="完全重复">完全重复</option>
    <option value="图片相似">图片相似</option>
    <option value="音频相似">音频相似</option>
    <option value="压缩包内重复">压缩包内重复</option>
    <option value="RAW/JPEG 配对">RAW/JPEG 配对</option>
  </select>
  <select id="decision">
    <option value="">全部决策</option>
    <option value="待清理">仅待清理</option>
    <option value="保留">仅保留</option>
  </select>
</div>

<table id="tbl">
<thead><tr>
  <th data-i="0">决策</th><th data-i="1">匹配类型</th><th data-i="2">文件名</th>
  <th data-i="3">体积</th><th data-i="4">修改时间</th><th data-i="5">完整路径</th><th data-i="6">判定理由</th>
</tr></thead>
<tbody>
"#,
        created = esc(&format_time(result.created_at)),
        algo = esc(result.algorithm.as_str()),
        group_count = result.stats.groups,
        waste = waste,
        dupes = result.stats.duplicate_files,
        seen = result.stats.files_seen,
        scanned = result.stats.files_scanned,
        cache = result.stats.cache_hits,
        duration = format!("{:.1}s", result.stats.duration_ms as f64 / 1000.0),
    )?;

    for group in &result.groups {
        write!(
            out,
            r#"<tr class="group-head"><td colspan="7">{} · 分组 {} · 可回收 {}{}</td></tr>"#,
            esc(&html_escape(&group.title)),
            esc(&group.id),
            human_bytes(group.wasted_bytes),
            group
                .similarity
                .map(|s| format!(" · 相似度 {:.1}%", s * 100.0))
                .unwrap_or_default()
        )?;

        for file in &group.files {
            let row_class = match file.decision {
                MarkDecision::Keep => "keep",
                MarkDecision::Remove => "remove",
                MarkDecision::Undecided => "",
            };
            let tag_class = match file.decision {
                MarkDecision::Keep => "keep",
                MarkDecision::Remove => "remove",
                MarkDecision::Undecided => "",
            };
            let reasons = if file.protected.is_some() {
                format!("🔒 {}", file.reasons.join("；"))
            } else {
                file.reasons.join("；")
            };

            write!(
                out,
                r#"<tr class="{row_class}"><td><span class="tag {tag_class}">{decision}</span></td><td>{kind}</td><td>{name}<div class="muted">{ext}</div></td><td>{size}</td><td class="muted">{mtime}</td><td class="path">{path}</td><td class="reasons">{reasons}</td></tr>"#,
                row_class = row_class,
                tag_class = tag_class,
                decision = decision_label(file.decision),
                kind = esc(&group.kind_label),
                name = esc(file.entry.file_name()),
                ext = esc(&file.entry.extension),
                size = human_bytes(file.entry.size),
                mtime = esc(&format_time(file.entry.modified)),
                path = esc(&file.entry.path),
                reasons = esc(&reasons),
            )?;
        }
    }

    write!(
        out,
        r#"</tbody>
</table>

<footer>
  由 FileClean 生成 · 本报告仅作审计与人工复核用途，FileClean 不会依据报告自动执行任何删除。
  {nothing_hint}
</footer>

<script>
(function () {{
  var tbl = document.getElementById('tbl');
  var q = document.getElementById('q');
  var kind = document.getElementById('kind');
  var decision = document.getElementById('decision');
  var rows = Array.prototype.slice.call(tbl.tBodies[0].rows);

  function refresh() {{
    var text = (q.value || '').toLowerCase();
    var k = kind.value, d = decision.value;
    var showGroup = true;
    rows.forEach(function (r) {{
      if (r.classList.contains('group-head')) {{
        showGroup = true;
        return;
      }}
      var cells = r.cells;
      var ok = true;
      if (text && r.innerText.toLowerCase().indexOf(text) === -1) ok = false;
      if (k && cells[1].innerText !== k) ok = false;
      if (d && cells[0].innerText.trim() !== d) ok = false;
      r.style.display = ok ? '' : 'none';
      if (ok) showGroup = false;
      r.dataset.hidden = ok ? '0' : '1';
    }});
    // 分组标题在组内全被隐藏时一并隐藏
    rows.forEach(function (r, i) {{
      if (!r.classList.contains('group-head')) return;
      var visible = false;
      for (var j = i + 1; j < rows.length; j++) {{
        if (rows[j].classList.contains('group-head')) break;
        if (rows[j].dataset.hidden === '0') {{ visible = true; break; }}
      }}
      r.style.display = visible ? '' : 'none';
    }});
  }}

  [q, kind, decision].forEach(function (el) {{ el.addEventListener('input', refresh); }});

  Array.prototype.forEach.call(tbl.tHead.rows[0].cells, function (th) {{
    th.addEventListener('click', function () {{
      var i = parseInt(th.dataset.i, 10);
      var asc = th.dataset.asc !== '1';
      th.dataset.asc = asc ? '1' : '0';
      var sorted = rows.slice().sort(function (a, b) {{
        if (a.classList.contains('group-head') || b.classList.contains('group-head')) return 0;
        var x = a.cells[i] ? a.cells[i].innerText : '';
        var y = b.cells[i] ? b.cells[i].innerText : '';
        return asc ? x.localeCompare(y, 'zh') : y.localeCompare(x, 'zh');
      }});
      var body = tbl.tBodies[0];
      sorted.forEach(function (r) {{ body.appendChild(r); }});
      refresh();
    }});
  }});
}})();
</script>
</body>
</html>
"#,
        nothing_hint = if total_waste == 0 {
            "本次未发现可回收的重复文件。"
        } else {
            ""
        }
    )?;

    Ok(())
}

fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_drops_illegal_control_chars() {
        let out = esc("a\u{0007}b");
        assert_eq!(out, "ab");
    }

    #[test]
    fn escape_handles_special_chars() {
        assert_eq!(esc("<a & 'b' \"c\">"), "&lt;a &amp; &apos;b&apos; &quot;c&quot;&gt;");
    }

    #[test]
    fn human_bytes_scales() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(2048), "2.00 KB");
        assert_eq!(human_bytes(5 * 1024 * 1024), "5.00 MB");
    }
}
