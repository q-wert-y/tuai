//! 通用工具：数据目录、宽度计算、折行、fuzzy 匹配、代码块提取等。

use std::path::PathBuf;
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

/// 数据目录：主程序（可执行文件）所在目录下的 `.tuai/`，不存在则自动创建。
pub fn data_dir() -> anyhow::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let dir = exe
        .parent()
        .ok_or_else(|| anyhow::anyhow!("无法获取可执行文件所在目录"))?
        .join(".tuai");
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// 检测代理地址（每次请求前调用，跟随系统代理开关即时生效）。
/// 优先级：环境变量（HTTPS_PROXY/HTTP_PROXY/ALL_PROXY）> Windows 系统代理。
/// 都没有则返回 None（直连）。
pub fn detect_proxy() -> Option<String> {
    for k in [
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        if let Ok(v) = std::env::var(k) {
            let v = v.trim().to_string();
            if !v.is_empty() {
                return Some(normalize_proxy(&v));
            }
        }
    }
    #[cfg(windows)]
    {
        windows_system_proxy().map(|s| normalize_proxy(&s))
    }
    #[cfg(not(windows))]
    None
}

/// 统一补上 http:// 前缀（注册表/环境变量里可能只写 host:port）。
fn normalize_proxy(addr: &str) -> String {
    if addr.contains("://") {
        addr.to_string()
    } else {
        format!("http://{addr}")
    }
}

/// 读取 Windows 系统代理（注册表，Clash 等开“系统代理”时会写入）。
#[cfg(windows)]
fn windows_system_proxy() -> Option<String> {
    use std::process::Command;
    // reg query 输出为本地编码，仅解析 ASCII 部分即可
    let out = Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
            "/v",
            "ProxyEnable",
        ])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    if !text.contains("0x1") {
        return None; // 系统代理未启用
    }
    let out = Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings",
            "/v",
            "ProxyServer",
        ])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    // 行格式：ProxyServer    REG_SZ    127.0.0.1:7890
    let line = text.lines().find(|l| l.contains("ProxyServer"))?;
    let server = line.split_whitespace().last()?;
    // 形如 "http=...;https=..." 时取 https 项，否则整体即 host:port
    if server.contains('=') {
        for part in server.split(';') {
            if let Some(v) = part
                .strip_prefix("https=")
                .or_else(|| part.strip_prefix("http="))
            {
                return Some(v.to_string());
            }
        }
        return None;
    }
    if server.is_empty() {
        None
    } else {
        Some(server.to_string())
    }
}

/// 初始化文件日志（`.tuai/logs/tuai.log`）。失败时静默降级（不影响主程序）。
pub fn init_logging(data_dir: &std::path::Path) {
    let log_dir = data_dir.join("logs");
    if std::fs::create_dir_all(&log_dir).is_err() {
        return;
    }
    let appender = tracing_appender::rolling::never(log_dir, "tuai.log");
    // 失败时静默降级（不影响主程序）
    let _ = tracing_subscriber::fmt()
        .with_writer(appender)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
}

/// 当前 Unix 秒级时间戳。
pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 计算字符串显示宽度（CJK/emoji 按 2 列计）。
pub fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

/// 按显示宽度截断字符串，超长补 `…`。
pub fn truncate_by_width(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if display_width(s) <= max {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0usize;
    for ch in s.chars() {
        let cw = ch.width().unwrap_or(0);
        if w + cw > max.saturating_sub(1) {
            break;
        }
        out.push(ch);
        w += cw;
    }
    out.push('…');
    out
}

/// 将单行文本按显示宽度折为多段，返回 (段文本, 起始字符索引, 结束字符索引)。
/// 用于渲染折行与光标定位。
pub fn wrap_segments(line: &str, width: usize) -> Vec<(String, usize, usize)> {
    let width = width.max(1);
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0usize;
    let mut start = 0usize;
    let mut end = 0usize;
    for (i, ch) in line.char_indices() {
        let cw = ch.width().unwrap_or(0);
        // 零宽字符（如组合符号）不触发换行
        if cur_w + cw > width && cur_w > 0 {
            out.push((std::mem::take(&mut cur), start, end));
            start = i;
            cur_w = 0;
        }
        cur.push(ch);
        cur_w += cw;
        end = i + ch.len_utf8();
    }
    out.push((cur, start, end));
    out
}

/// 将多行文本折为多行字符串（每行不超过 width 显示宽度）。
pub fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.split('\n') {
        for (seg, _, _) in wrap_segments(line, width) {
            out.push(seg.trim_end().to_string());
        }
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

/// 简单 fuzzy 匹配（大小写不敏感的子序列匹配），返回得分（越高越靠前）。
pub fn fuzzy_score(hay: &str, needle: &str) -> Option<i32> {
    if needle.is_empty() {
        return Some(0);
    }
    let hay: Vec<char> = hay.to_lowercase().chars().collect();
    let needle: Vec<char> = needle.to_lowercase().chars().collect();
    let mut score = 0i32;
    let mut last_match: Option<usize> = None;
    let mut hi = 0usize;
    for &n in &needle {
        let mut found = None;
        while hi < hay.len() {
            if hay[hi] == n {
                found = Some(hi);
                hi += 1;
                break;
            }
            hi += 1;
        }
        let pos = found?;
        score += match last_match {
            Some(p) if p + 1 == pos => 3, // 连续匹配
            _ => 1,
        };
        last_match = Some(pos);
    }
    Some(score)
}

/// 提取文本中最后一个围栏代码块（``` 包裹）的内容。
pub fn extract_last_code_block(text: &str) -> Option<String> {
    let mut blocks: Vec<String> = Vec::new();
    let mut in_block = false;
    let mut cur = String::new();
    let mut fence_len = 3usize;
    for line in text.lines() {
        let trimmed = line.trim_start();
        let is_fence = trimmed.starts_with("```") || trimmed.starts_with("~~~");
        if is_fence {
            let fl = trimmed
                .chars()
                .take_while(|c| *c == '`' || *c == '~')
                .count();
            if !in_block {
                in_block = true;
                fence_len = fl;
                cur.clear();
            } else if fl >= fence_len {
                in_block = false;
                fence_len = 3;
                blocks.push(std::mem::take(&mut cur));
            } else {
                cur.push_str(line);
                cur.push('\n');
            }
        } else if in_block {
            cur.push_str(line);
            cur.push('\n');
        }
    }
    blocks.pop().map(|b| b.trim_end().to_string())
}

/// 文件名安全化（移除路径非法字符）。
pub fn sanitize_filename(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => ' ',
            _ => c,
        })
        .collect();
    cleaned.trim().replace(' ', "-")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_cjk() {
        let lines = wrap_text("你好世界世界", 8);
        assert_eq!(lines, vec!["你好世界".to_string(), "世界".to_string()]);
    }

    #[test]
    fn wrap_mixed() {
        let lines = wrap_text("abc 你好", 5);
        assert_eq!(lines, vec!["abc".to_string(), "你好".to_string()]);
    }

    #[test]
    fn wrap_newline() {
        let lines = wrap_text("a\nb", 10);
        assert_eq!(lines, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn fuzzy_basic() {
        assert!(fuzzy_score("New Session", "ns").is_some());
        assert!(fuzzy_score("New Session", "xyz").is_none());
        assert!(fuzzy_score("abc", "abc").unwrap() > fuzzy_score("abc", "ac").unwrap());
    }

    #[test]
    fn code_block() {
        let text = "前文\n```rust\nfn main() {}\n```\n后文\n```py\nx = 1\n```";
        assert_eq!(extract_last_code_block(text), Some("x = 1".to_string()));
        assert_eq!(extract_last_code_block("无代码"), None);
    }

    #[test]
    fn truncate() {
        assert_eq!(truncate_by_width("你好世界", 4), "你…");
        assert_eq!(truncate_by_width("ab", 4), "ab");
    }
}
