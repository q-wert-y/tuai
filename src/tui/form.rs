//! 模态表单（提供商添加/编辑、重命名会话、系统提示词）。

use crate::app::App;
use crate::tui::layout;
use crate::util::{display_width, truncate_by_width};
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;
use unicode_width::UnicodeWidthChar;

pub fn render(f: &mut Frame, area: Rect, app: &App) -> Option<(u16, u16)> {
    let Some(form) = &app.form else {
        return None;
    };
    let theme = &app.theme;
    let rect = layout::centered(70, 50, area);
    f.render_widget(Clear, rect);
    let block = Block::bordered()
        .title(Span::styled(
            format!(" {} ", form.title),
            Style::new().fg(theme.accent),
        ))
        .border_style(Style::new().fg(theme.accent));
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let mut lines = Vec::new();
    let mut label_widths = Vec::new();
    let mut field_y = Vec::new();
    for (i, fld) in form.fields.iter().enumerate() {
        field_y.push(lines.len() as u16);
        let sel = i == form.idx;
        let label = if sel {
            format!("▸ {} ", fld.label)
        } else {
            format!("  {} ", fld.label)
        };
        let label_w = display_width(&label);
        let value = if fld.secret {
            "*".repeat(fld.value.chars().count())
        } else {
            fld.value.clone()
        };
        let style = if sel {
            Style::new().fg(theme.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(theme.dim)
        };
        let vw = (inner.width as usize).saturating_sub(label_w + 2);
        // 多行值：逐行渲染（超出视口高度则仅保留尾部）
        let vlines: Vec<String> = value
            .split('\n')
            .map(|l| truncate_by_width(l, vw))
            .collect();
        let vlines: Vec<String> = vlines
            .into_iter()
            .rev()
            .take(inner.height.saturating_sub(2) as usize)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        for (li, v) in vlines.iter().enumerate() {
            let pad = if li == 0 {
                String::new()
            } else {
                " ".repeat(label_w)
            };
            lines.push(Line::from(vec![
                Span::styled(if li == 0 { label.clone() } else { pad }, style),
                Span::raw(v.clone()),
            ]));
        }
        label_widths.push(label_w);
    }
    // 底部提示行
    while lines.len() >= inner.height as usize {
        lines.remove(0);
        for y in &mut field_y {
            *y = y.saturating_sub(1);
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!(" {}", form.hint),
        Style::new().fg(theme.faint),
    )));
    f.render_widget(Paragraph::new(lines), inner);

    // 光标：当前字段值内、按字符位置折算显示列（多行值折算行号）
    let fld = form.field();
    let byte = fld
        .value
        .char_indices()
        .nth(fld.cx)
        .map(|(b, _)| b)
        .unwrap_or(fld.value.len());
    let before = &fld.value[..byte];
    let line_idx = before.matches('\n').count() as u16;
    let cur_line = before.split('\n').next_back().unwrap_or("");
    let vw: usize = cur_line.chars().map(|c| c.width().unwrap_or(0)).sum();
    let x = inner.x + (label_widths[form.idx] + vw) as u16;
    let x = x.min(inner.x + inner.width.saturating_sub(1));
    let y = inner.y + field_y[form.idx] + line_idx;
    Some((x, y.min(inner.y + inner.height.saturating_sub(1))))
}
