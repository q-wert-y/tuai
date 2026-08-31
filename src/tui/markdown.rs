//! Markdown → ratatui Line 渲染（含 syntect 代码高亮）。

use crate::tui::theme::Theme;
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::sync::OnceLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::Theme as SynTheme;
use syntect::parsing::{SyntaxReference, SyntaxSet};

struct SyntaxAssets {
    set: SyntaxSet,
    theme: SynTheme,
}

fn assets() -> &'static SyntaxAssets {
    static ASSETS: OnceLock<SyntaxAssets> = OnceLock::new();
    ASSETS.get_or_init(|| SyntaxAssets {
        set: SyntaxSet::load_defaults_newlines(),
        theme: syntect::highlighting::ThemeSet::load_defaults()
            .themes
            .get("base16-ocean.dark")
            .cloned()
            .unwrap_or_else(|| {
                syntect::highlighting::ThemeSet::load_defaults()
                    .themes
                    .values()
                    .next()
                    .cloned()
                    .unwrap()
            }),
    })
}

/// syntect 前景色 → ratatui 颜色。
fn to_color(c: syntect::highlighting::Color) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}

/// 代码块高亮渲染（带围栏边框与语言标签）。
fn code_block(code: &str, lang: &str, width: usize, theme: &Theme) -> Vec<Line<'static>> {
    let inner_w = width.saturating_sub(4).max(1); // "│ " + 内容 + " "
    let label = if lang.is_empty() {
        "code".to_string()
    } else {
        lang.to_string()
    };
    let mut lines = Vec::new();

    let mut top = format!("╭─ {label} ");
    let top_w = crate::util::display_width(&top);
    if width > top_w {
        top.push_str(&"─".repeat(width - top_w));
        top.push('╮');
    }
    lines.push(Line::from(Span::styled(top, Style::new().fg(theme.code))));

    if let Some(syntax) = find_syntax(lang) {
        let assets = assets();
        let mut hl = HighlightLines::new(syntax, &assets.theme);
        for raw in code.lines() {
            let mut spans: Vec<Span<'static>> =
                vec![Span::styled("│ ".to_string(), Style::new().fg(theme.code))];
            if let Ok(ranges) = hl.highlight_line(raw, &assets.set) {
                for (style, text) in ranges {
                    let text = crate::util::truncate_by_width(text, inner_w);
                    if text.is_empty() {
                        continue;
                    }
                    let mut st = Style::new().fg(to_color(style.foreground));
                    if style
                        .font_style
                        .contains(syntect::highlighting::FontStyle::BOLD)
                    {
                        st = st.add_modifier(Modifier::BOLD);
                    }
                    if style
                        .font_style
                        .contains(syntect::highlighting::FontStyle::ITALIC)
                    {
                        st = st.add_modifier(Modifier::ITALIC);
                    }
                    spans.push(Span::styled(text, st));
                }
            }
            lines.push(Line::from(spans));
        }
    } else {
        for raw in crate::util::wrap_text(code, inner_w) {
            lines.push(Line::from(vec![
                Span::styled("│ ".to_string(), Style::new().fg(theme.code)),
                Span::raw(raw),
            ]));
        }
    }

    let mut bottom = String::from("╰");
    if width > 2 {
        bottom.push_str(&"─".repeat(width - 2));
        bottom.push('╯');
    }
    lines.push(Line::from(Span::styled(
        bottom,
        Style::new().fg(theme.code),
    )));
    lines
}

fn find_syntax(lang: &str) -> Option<&'static SyntaxReference> {
    let set = &assets().set;
    let lang = lang.trim().to_lowercase();
    if lang.is_empty() {
        return None;
    }
    set.find_syntax_by_token(&lang)
        .or_else(|| set.find_syntax_by_extension(&lang))
}

// ---------- Markdown 状态机 ----------

#[derive(Default)]
struct TableAcc {
    header: Vec<String>,
    rows: Vec<Vec<String>>,
    cur_row: Vec<String>,
    cur_cell: String,
    in_head: bool,
}

struct Ctx<'a> {
    theme: &'a Theme,
    width: usize,
    out: Vec<Line<'static>>,
    /// 当前行的 span 缓冲
    spans: Vec<Span<'static>>,
    prefix: String,
    bold: bool,
    italic: bool,
    strike: bool,
    quote: u32,
    heading: bool,
    table: Option<TableAcc>,
    code: Option<(String, String)>, // (lang, content)
    list_stack: Vec<Option<u64>>,
}

impl<'a> Ctx<'a> {
    fn text_style(&self) -> Style {
        let mut st = Style::new();
        if self.bold {
            st = st.add_modifier(Modifier::BOLD);
        }
        if self.italic {
            st = st.add_modifier(Modifier::ITALIC);
        }
        if self.strike {
            st = st.add_modifier(Modifier::CROSSED_OUT);
        }
        if self.quote > 0 || self.heading {
            st = st.fg(self.theme.dim);
        }
        st
    }

    fn push_text(&mut self, t: &str) {
        if let Some((_, content)) = &mut self.code {
            content.push_str(t);
            return;
        }
        if let Some(table) = &mut self.table {
            if t.contains('\n') {
                // 单元格内换行折叠为空格
                table.cur_cell.push(' ');
            } else {
                table.cur_cell.push_str(t);
            }
            return;
        }
        let st = self.text_style();
        if self.spans.is_empty() && !self.prefix.is_empty() {
            let p = std::mem::take(&mut self.prefix);
            let pst = if self.quote > 0 {
                Style::new().fg(self.theme.quote)
            } else {
                Style::new().fg(self.theme.accent)
            };
            self.spans.push(Span::styled(p, pst));
        }
        self.spans.push(Span::styled(t.to_string(), st));
    }

    /// 结束当前行（段落/列表项/标题结尾调用）。
    fn flush_line(&mut self) {
        if !self.spans.is_empty() {
            self.out.push(Line::from(std::mem::take(&mut self.spans)));
        }
        self.prefix.clear();
        self.bold = false;
        self.italic = false;
        self.strike = false;
    }

    fn blank(&mut self) {
        if let Some(last) = self.out.last() {
            if !last.spans.is_empty() {
                self.out.push(Line::from(""));
            }
        }
    }

    fn push_prefix(&mut self, p: &str) {
        if self.spans.is_empty() {
            self.prefix.push_str(p);
        }
    }
}

fn indent_for(depth: usize) -> String {
    "  ".repeat(depth)
}

/// 渲染 Markdown 文本为带样式的行集合。纯文本将原样折行显示。
pub fn render(text: &str, width: usize, theme: &Theme) -> Vec<Line<'static>> {
    let width = width.max(8);
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(text, opts);

    let mut ctx = Ctx {
        theme,
        width,
        out: Vec::new(),
        spans: Vec::new(),
        prefix: String::new(),
        bold: false,
        italic: false,
        strike: false,
        quote: 0,
        heading: false,
        table: None,
        code: None,
        list_stack: Vec::new(),
    };

    for ev in parser {
        match ev {
            Event::Start(tag) => start_tag(&mut ctx, tag),
            Event::End(tag) => end_tag(&mut ctx, tag),
            Event::Text(t) => ctx.push_text(&t),
            Event::Code(t) => {
                // 行内代码
                let st = Style::new().fg(theme.accent).bg(Color::Rgb(46, 52, 64));
                if ctx.spans.is_empty() && !ctx.prefix.is_empty() {
                    let p = std::mem::take(&mut ctx.prefix);
                    ctx.spans
                        .push(Span::styled(p, Style::new().fg(theme.accent)));
                }
                ctx.spans.push(Span::styled(format!(" {t} "), st));
            }
            Event::SoftBreak => ctx.push_text(" "),
            Event::HardBreak => {
                // 硬换行：结束当前行，并保留列表/引用前缀到下一行
                let prefix_keep = current_block_prefix(&ctx);
                ctx.flush_line();
                ctx.prefix = prefix_keep;
            }
            Event::Rule => {
                ctx.flush_line();
                let line = "─".repeat(ctx.width.saturating_sub(2));
                ctx.out
                    .push(Line::from(Span::styled(line, Style::new().fg(theme.faint))));
                ctx.blank();
            }
            Event::TaskListMarker(checked) => {
                let marker = if checked { "[x] " } else { "[ ] " };
                ctx.push_prefix(marker);
            }
            _ => {}
        }
    }
    ctx.flush_line();
    ctx.out
}

/// 计算当前块的前缀（列表缩进 / 引用符），用于硬换行后续行。
fn current_block_prefix(ctx: &Ctx) -> String {
    let mut p = indent_for(ctx.list_stack.len());
    if ctx.quote > 0 {
        p.push_str("│ ");
    }
    p
}

fn start_tag(ctx: &mut Ctx, tag: Tag) {
    match tag {
        Tag::Paragraph => {}
        Tag::Heading { .. } => {
            ctx.heading = true;
            ctx.push_prefix("◆ ");
        }
        Tag::BlockQuote(_) => {
            ctx.quote += 1;
            ctx.push_prefix("│ ");
        }
        Tag::CodeBlock(kind) => {
            ctx.flush_line();
            let lang = match kind {
                CodeBlockKind::Fenced(info) => {
                    info.split([' ', ',']).next().unwrap_or("").to_string()
                }
                CodeBlockKind::Indented => String::new(),
            };
            ctx.code = Some((lang, String::new()));
        }
        Tag::List(start) => {
            ctx.list_stack.push(start);
            ctx.blank();
        }
        Tag::Item => {
            ctx.flush_line();
            let depth = ctx.list_stack.len().saturating_sub(1);
            match ctx.list_stack.last().copied().flatten() {
                Some(n) => ctx.prefix = format!("{}{}. ", indent_for(depth), n),
                None => ctx.prefix = format!("{}• ", indent_for(depth)),
            }
            if let Some(Some(n)) = ctx.list_stack.last_mut() {
                *n += 1;
            }
        }
        Tag::Emphasis => ctx.italic = true,
        Tag::Strong => ctx.bold = true,
        Tag::Strikethrough => ctx.strike = true,
        Tag::Table(_) => {
            ctx.table = Some(TableAcc {
                header: Vec::new(),
                rows: Vec::new(),
                cur_row: Vec::new(),
                cur_cell: String::new(),
                in_head: false,
            });
        }
        Tag::TableHead => {
            if let Some(t) = &mut ctx.table {
                t.in_head = true;
            }
        }
        Tag::TableRow => {}
        Tag::TableCell => {}
        _ => {}
    }
}

fn end_tag(ctx: &mut Ctx, tag: TagEnd) {
    match tag {
        TagEnd::Paragraph => {
            ctx.flush_line();
            ctx.blank();
        }
        TagEnd::Heading(_) => {
            // 标题整行加粗 + 强调色
            if let Some(line) = ctx.out.last_mut() {
                for span in &mut line.spans {
                    span.style = span.style.fg(ctx.theme.accent).add_modifier(Modifier::BOLD);
                }
            }
            ctx.heading = false;
            ctx.blank();
        }
        TagEnd::BlockQuote => {
            ctx.quote = ctx.quote.saturating_sub(1);
            ctx.flush_line();
            ctx.blank();
        }
        TagEnd::CodeBlock => {
            if let Some((lang, code)) = ctx.code.take() {
                ctx.blank();
                let block = code_block(&code, &lang, ctx.width, ctx.theme);
                ctx.out.extend(block);
                ctx.blank();
            }
        }
        TagEnd::List(_) => {
            ctx.list_stack.pop();
            ctx.blank();
        }
        TagEnd::Item => ctx.flush_line(),
        TagEnd::Emphasis => ctx.italic = false,
        TagEnd::Strong => ctx.bold = false,
        TagEnd::Strikethrough => ctx.strike = false,
        TagEnd::TableHead => {
            if let Some(t) = &mut ctx.table {
                t.header = std::mem::take(&mut t.cur_row);
                t.in_head = false;
            }
        }
        TagEnd::TableRow => {
            if let Some(t) = &mut ctx.table {
                let row = std::mem::take(&mut t.cur_row);
                if !row.is_empty() {
                    t.rows.push(row);
                }
            }
        }
        TagEnd::TableCell => {
            if let Some(t) = &mut ctx.table {
                t.cur_row
                    .push(std::mem::take(&mut t.cur_cell).trim().to_string());
            }
        }
        TagEnd::Table => {
            if let Some(t) = ctx.table.take() {
                let table = render_table(&t, ctx.width, ctx.theme);
                ctx.out.extend(table);
                ctx.blank();
            }
        }
        _ => {}
    }
}

fn render_table(t: &TableAcc, width: usize, theme: &Theme) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    if t.header.is_empty() && t.rows.is_empty() {
        return out;
    }
    let ncol = t
        .header
        .len()
        .max(t.rows.iter().map(|r| r.len()).max().unwrap_or(0));
    if ncol == 0 {
        return out;
    }
    // 每列宽度：max 单元格宽度，上限 (width-2) / ncol
    let col_cap = ((width.saturating_sub(2)) / ncol).max(4);
    let mut widths = vec![1usize; ncol];
    for (i, w) in widths.iter_mut().enumerate() {
        let hw = t
            .header
            .get(i)
            .map(|c| crate::util::display_width(c))
            .unwrap_or(0);
        *w = hw.min(col_cap) + 2;
        for row in &t.rows {
            let cw = row
                .get(i)
                .map(|c| crate::util::display_width(c))
                .unwrap_or(0);
            *w = (*w).max(cw.min(col_cap) + 2);
        }
    }

    let mut header_spans = vec![Span::styled("│".to_string(), Style::new().fg(theme.faint))];
    for (i, w) in widths.iter().enumerate() {
        let cell = t.header.get(i).map(|s| s.as_str()).unwrap_or("");
        let text = crate::util::truncate_by_width(cell, w - 2);
        let pad = w.saturating_sub(crate::util::display_width(&text)) / 2;
        let text = format!(
            "{}{}{}",
            " ".repeat(pad),
            text,
            " ".repeat(w - pad - crate::util::display_width(&text))
        );
        header_spans.push(Span::styled(
            format!(" {text} "),
            Style::new().fg(theme.dim).add_modifier(Modifier::BOLD),
        ));
        header_spans.push(Span::styled("│".to_string(), Style::new().fg(theme.faint)));
    }
    out.push(Line::from(header_spans));

    // 分隔线
    let sep: String = widths
        .iter()
        .map(|w| format!("{}+", "─".repeat(w + 2)))
        .collect();
    let sep = format!("+{}", sep.trim_end_matches('+'));
    out.push(Line::from(Span::styled(sep, Style::new().fg(theme.faint))));

    for row in &t.rows {
        let mut spans = vec![Span::styled("│".to_string(), Style::new().fg(theme.faint))];
        for (i, w) in widths.iter().enumerate() {
            let cell = row.get(i).map(|s| s.as_str()).unwrap_or("");
            let text = crate::util::truncate_by_width(cell, w - 2);
            let pad = w.saturating_sub(crate::util::display_width(&text));
            let text = format!(" {text}{}", " ".repeat(pad));
            spans.push(Span::raw(text));
            spans.push(Span::styled("│".to_string(), Style::new().fg(theme.faint)));
        }
        out.push(Line::from(spans));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_plain_text() {
        let theme = Theme::default();
        let lines = render("你好，世界", 40, &theme);
        assert!(!lines.is_empty());
        let joined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect();
        assert!(joined.contains("你好，世界"));
    }

    #[test]
    fn renders_code_block() {
        let theme = Theme::default();
        let lines = render("```rust\nfn main() {}\n```", 40, &theme);
        let joined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect();
        assert!(joined.contains("rust"));
        assert!(joined.contains("fn main() {}"));
        assert!(joined.contains('╰'));
    }

    #[test]
    fn renders_list() {
        let theme = Theme::default();
        let lines = render("- a\n- b", 40, &theme);
        let joined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect();
        assert!(joined.contains("• a"));
        assert!(joined.contains("• b"));
    }

    #[test]
    fn renders_heading() {
        let theme = Theme::default();
        let lines = render("# 标题", 40, &theme);
        let joined: String = lines
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect();
        assert!(joined.contains("◆ 标题"));
    }
}
