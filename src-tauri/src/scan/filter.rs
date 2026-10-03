//! 过滤系统：扩展名 / 体积 / 时间 / 隐藏属性 / glob / 系统目录保护锁。

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

use crate::domain::models::{ScanFilter, JUNK_EXTENSIONS, JUNK_FILE_NAMES};
use crate::error::AppResult;

/// 结构性垃圾目录：无论用户怎么配置都不会有清理价值，直接剪枝。
/// 注意不包含 `Library/Caches` 之类用户可自行取舍的目录 —— 那些走 glob 规则。
const ALWAYS_PRUNE_DIRS: &[&str] = &[
    ".git",
    ".svn",
    ".hg",
    "node_modules",
    ".Trash",
    ".Trashes",
    "$RECYCLE.BIN",
    "System Volume Information",
    ".Spotlight-V100",
    ".fseventsd",
    ".DocumentRevisions-V100",
    ".TemporaryItems",
    ".PKInstallSandboxManager",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    SystemProtected,
    StructuralJunk,
    ExcludedGlob,
    Hidden,
    Symlink,
}

impl SkipReason {
    pub fn label(self) -> &'static str {
        match self {
            SkipReason::SystemProtected => "系统关键目录（保护锁）",
            SkipReason::StructuralJunk => "结构性垃圾目录（如 node_modules / .git）",
            SkipReason::ExcludedGlob => "命中排除规则",
            SkipReason::Hidden => "隐藏目录",
            SkipReason::Symlink => "符号链接目录",
        }
    }
}

/// 系统关键目录保护锁。
///
/// 即使用户把 `/` 或整个家目录选为扫描根，这些位置也不会被遍历，
/// 从根上杜绝「删掉系统组件导致系统或软件崩溃」。
pub struct ProtectedPaths {
    enabled: bool,
    prefixes: Vec<PathBuf>,
}

impl ProtectedPaths {
    pub fn new(enabled: bool) -> Self {
        let mut prefixes: Vec<PathBuf> = Vec::new();

        #[cfg(target_os = "macos")]
        {
            // 注意：不能整目录保护 `/private` —— `/tmp` 与 `/etc` 都是它的软链，
            // 整块封掉会让用户连临时目录都扫不了。这里只锁定真正的系统状态位置。
            for p in [
                "/System",
                "/bin",
                "/sbin",
                "/usr",
                "/dev",
                "/cores",
                "/Library",
                "/Applications",
                "/System/Volumes",
                "/private/etc",
                "/private/var",
                "/Volumes/.Spotlight-V100",
            ] {
                prefixes.push(PathBuf::from(p));
            }
            if let Ok(home) = std::env::var("HOME") {
                for p in ["Library", ".Trash", ".ssh", ".gnupg", ".DocumentRevisions-V100"] {
                    prefixes.push(PathBuf::from(&home).join(p));
                }
            }
        }

        #[cfg(target_os = "windows")]
        {
            if let Ok(sysroot) = std::env::var("SystemRoot") {
                prefixes.push(PathBuf::from(&sysroot));
            }
            for p in ["C:\\Windows", "C:\\Program Files", "C:\\Program Files (x86)", "C:\\ProgramData", "C:\\$Recycle.Bin"] {
                prefixes.push(PathBuf::from(p));
            }
            if let Ok(appdata) = std::env::var("APPDATA") {
                prefixes.push(PathBuf::from(&appdata).join("Microsoft").join("Windows"));
            }
        }

        #[cfg(all(unix, not(target_os = "macos")))]
        {
            for p in ["/proc", "/sys", "/dev", "/boot", "/etc", "/usr", "/bin", "/sbin", "/lib", "/lib64", "/srv", "/run"] {
                prefixes.push(PathBuf::from(p));
            }
            if let Ok(home) = std::env::var("HOME") {
                for p in [".ssh", ".gnupg", ".local/share/Trash"] {
                    prefixes.push(PathBuf::from(&home).join(p));
                }
            }
        }

        ProtectedPaths { enabled, prefixes }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// 路径本身或其任一祖先位于保护清单内即为受保护。
    pub fn is_protected(&self, path: &Path) -> bool {
        if !self.enabled {
            return false;
        }
        self.prefixes.iter().any(|prefix| path.starts_with(prefix))
    }

    pub fn prefixes(&self) -> &[PathBuf] {
        &self.prefixes
    }
}

pub struct FilterEngine {
    include_ext: HashSet<String>,
    exclude_ext: HashSet<String>,
    globs: GlobSet,
    filter: ScanFilter,
    protected: ProtectedPaths,
}

impl FilterEngine {
    pub fn new(filter: &ScanFilter, protect_system_dirs: bool) -> AppResult<Self> {
        let mut builder = GlobSetBuilder::new();
        for pattern in &filter.exclude_globs {
            let trimmed = pattern.trim();
            if trimmed.is_empty() {
                continue;
            }
            // 用户手写的 glob 可能不合法；单条失败不应让整次扫描挂掉。
            match GlobBuilder::new(trimmed).literal_separator(false).build() {
                Ok(glob) => {
                    builder.add(glob);
                }
                Err(e) => {
                    tracing::warn!(pattern = trimmed, error = %e, "忽略非法的排除 glob");
                }
            }
        }
        let globs = builder.build().unwrap_or_else(|_| GlobSet::empty());

        Ok(FilterEngine {
            include_ext: normalize_exts(&filter.include_extensions),
            exclude_ext: normalize_exts(&filter.exclude_extensions),
            globs,
            filter: filter.clone(),
            protected: ProtectedPaths::new(protect_system_dirs),
        })
    }

    pub fn protected(&self) -> &ProtectedPaths {
        &self.protected
    }

    pub fn filter(&self) -> &ScanFilter {
        &self.filter
    }

    /// 目录剪枝判断。返回 `Some(reason)` 表示不应进入该目录。
    pub fn should_prune_dir(&self, dir: &Path, root: &Path, is_symlink: bool) -> Option<SkipReason> {
        if self.protected.is_protected(dir) {
            return Some(SkipReason::SystemProtected);
        }
        if is_symlink && !self.filter.follow_symlinks {
            return Some(SkipReason::Symlink);
        }

        let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if ALWAYS_PRUNE_DIRS.iter().any(|j| j.eq_ignore_ascii_case(name)) {
            return Some(SkipReason::StructuralJunk);
        }
        if !self.filter.include_hidden && name.starts_with('.') && dir != root {
            return Some(SkipReason::Hidden);
        }

        if !self.globs.is_empty() {
            if let Ok(rel) = dir.strip_prefix(root) {
                let rel_str = rel.to_string_lossy().replace('\\', "/");
                if !rel_str.is_empty() && self.globs.is_match(rel_str.as_str()) {
                    return Some(SkipReason::ExcludedGlob);
                }
            }
        }
        None
    }

    /// 文件级判断。
    pub fn accept_file(&self, path: &Path, root: &Path, size: u64, modified: i64, depth: usize) -> bool {
        if depth < self.filter.min_depth {
            return false;
        }
        if size < self.filter.min_size {
            return false;
        }
        if let Some(max) = self.filter.max_size {
            if max > 0 && size > max {
                return false;
            }
        }
        if let Some(after) = self.filter.modified_after {
            if modified < after {
                return false;
            }
        }
        if let Some(before) = self.filter.modified_before {
            if modified > before {
                return false;
            }
        }

        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !self.filter.include_hidden && name.starts_with('.') {
            return false;
        }
        if !self.filter.include_system {
            if JUNK_FILE_NAMES.iter().any(|j| j.eq_ignore_ascii_case(name)) {
                return false;
            }
            if name.ends_with('~') || name.starts_with("._") {
                return false;
            }
        }

        let ext = extension_of(path);
        if JUNK_EXTENSIONS.contains(&ext.as_str()) {
            return false;
        }
        if self.exclude_ext.contains(&ext) {
            return false;
        }
        if !self.include_ext.is_empty() && !self.include_ext.contains(&ext) {
            return false;
        }

        if !self.globs.is_empty() {
            if let Ok(rel) = path.strip_prefix(root) {
                let rel_str = rel.to_string_lossy().replace('\\', "/");
                if self.globs.is_match(rel_str.as_str()) {
                    return false;
                }
            }
        }
        true
    }

    /// 扫描根自身是否位于保护范围内 —— 是则整次扫描应该被拒绝。
    pub fn check_root(&self, root: &Path) -> AppResult<()> {
        if self.protected.is_protected(root) {
            return Err(crate::error::AppError::forbidden(format!(
                "扫描根位于系统保护目录内，已拒绝：{}。如确需扫描，请关闭「系统关键目录保护锁」。",
                root.display()
            )));
        }
        Ok(())
    }
}

pub fn extension_of(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default()
}

/// 路径转成用于 UTF-8 存储的字符串（非 UTF-8 路径会被无损化为 lossy 形式）。
pub fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

/// 清理用户输入的扩展名：去掉前导点、统一小写。
fn normalize_exts(input: &[String]) -> HashSet<String> {
    input
        .iter()
        .map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase())
        .filter(|e| !e.is_empty())
        .collect()
}

/// 相对路径计算，失败时退回文件名。
pub fn relative_path(path: &Path, root: &Path) -> String {    match path.strip_prefix(root) {
        Ok(rel) => {
            let mut parts: Vec<String> = Vec::new();
            for comp in rel.components() {
                match comp {
                    Component::Normal(s) => parts.push(s.to_string_lossy().to_string()),
                    Component::ParentDir => parts.push("..".into()),
                    _ => {}
                }
            }
            if parts.is_empty() {
                path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
            } else {
                parts.join("/")
            }
        }
        Err(_) => path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::ScanFilter;

    #[test]
    fn extension_normalization_strips_dots_and_case() {
        let filter = ScanFilter {
            include_extensions: vec![".JPG".into(), " png ".into(), String::new()],
            ..Default::default()
        };
        let engine = FilterEngine::new(&filter, false).unwrap();
        assert!(engine.accept_file(Path::new("/r/a.jpg"), Path::new("/r"), 10, 1, 1));
        assert!(engine.accept_file(Path::new("/r/b.PNG"), Path::new("/r"), 10, 1, 1));
        assert!(!engine.accept_file(Path::new("/r/c.gif"), Path::new("/r"), 10, 1, 1));
    }

    #[test]
    fn size_and_time_filters_are_inclusive() {
        let filter = ScanFilter {
            min_size: 100,
            max_size: Some(200),
            modified_after: Some(1000),
            modified_before: Some(2000),
            ..Default::default()
        };
        let engine = FilterEngine::new(&filter, false).unwrap();
        assert!(engine.accept_file(Path::new("/r/a.bin"), Path::new("/r"), 100, 1000, 1));
        assert!(engine.accept_file(Path::new("/r/a.bin"), Path::new("/r"), 200, 2000, 1));
        assert!(!engine.accept_file(Path::new("/r/a.bin"), Path::new("/r"), 99, 1500, 1));
        assert!(!engine.accept_file(Path::new("/r/a.bin"), Path::new("/r"), 201, 1500, 1));
        assert!(!engine.accept_file(Path::new("/r/a.bin"), Path::new("/r"), 150, 999, 1));
        assert!(!engine.accept_file(Path::new("/r/a.bin"), Path::new("/r"), 150, 2001, 1));
    }

    #[test]
    fn junk_files_skipped_unless_system_files_included() {
        let engine = FilterEngine::new(&ScanFilter::default(), false).unwrap();
        assert!(!engine.accept_file(Path::new("/r/.DS_Store"), Path::new("/r"), 10, 1, 1));
        assert!(!engine.accept_file(Path::new("/r/file.tmp"), Path::new("/r"), 10, 1, 1));
        assert!(engine.accept_file(Path::new("/r/real.jpg"), Path::new("/r"), 10, 1, 1));

        let permissive = ScanFilter {
            include_hidden: true,
            include_system: true,
            ..Default::default()
        };
        let engine = FilterEngine::new(&permissive, false).unwrap();
        assert!(engine.accept_file(Path::new("/r/.DS_Store"), Path::new("/r"), 10, 1, 1));
    }

    #[test]
    fn structural_dirs_are_pruned() {
        let engine = FilterEngine::new(&ScanFilter::default(), false).unwrap();
        assert_eq!(
            engine.should_prune_dir(Path::new("/r/node_modules"), Path::new("/r"), false),
            Some(SkipReason::StructuralJunk)
        );
        assert_eq!(
            engine.should_prune_dir(Path::new("/r/.git"), Path::new("/r"), false),
            Some(SkipReason::StructuralJunk)
        );
        assert_eq!(
            engine.should_prune_dir(Path::new("/r/src"), Path::new("/r"), false),
            None
        );
    }

    #[test]
    fn glob_exclusion_applies_to_relative_path() {
        let filter = ScanFilter {
            exclude_globs: vec!["**/Caches/**".into(), "**/node_modules/**".into()],
            ..Default::default()
        };
        let engine = FilterEngine::new(&filter, false).unwrap();
        assert!(!engine.accept_file(
            Path::new("/r/App/Caches/big.bin"),
            Path::new("/r"),
            10,
            1,
            3
        ));
        assert!(engine.accept_file(Path::new("/r/App/data.bin"), Path::new("/r"), 10, 1, 3));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_protection_excludes_system_state_but_allows_tmp() {
        let protected = ProtectedPaths::new(true);
        // 系统状态目录必须锁住。
        assert!(protected.is_protected(Path::new("/private/etc/passwd")));
        assert!(protected.is_protected(Path::new("/private/var/db/anything")));
        assert!(protected.is_protected(Path::new("/System/Library/CoreServices")));
        // /tmp 是 /private/tmp 的软链：临时目录必须可扫，否则工具没法用。
        assert!(!protected.is_protected(Path::new("/private/tmp/fileclean-fixture")));
    }

    #[test]
    fn protection_can_be_disabled_entirely() {
        let protected = ProtectedPaths::new(false);
        assert!(!protected.is_enabled());
        assert!(!protected.is_protected(Path::new("/System/Library")));
    }
}
