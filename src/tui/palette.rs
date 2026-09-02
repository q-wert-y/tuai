//! 命令 / 模型 / 提供商选择面板（fuzzy 过滤）。

use crate::app::{App, PaletteKind};
use crate::tui::layout;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let Some(p) = &app.palette else { return };
    let theme = &app.theme;
    let title = match app.palette_kind {
        PaletteKind::Commands => " 命令面板 ",
        PaletteKind::Models => " 选择模型 ",
        PaletteKind::ProvidersEdit => " 编辑提供商 ",
        PaletteKind::ProvidersDelete => " 删除提供商 ",
        PaletteKind::ProvidersSwitch => " 切换提供商 ",
    };
    let rect = layout::centered(70, 50, area);
    f.render_widget(Clear, rect);
    let block = Block::bordered()
        .title(Span::styled(title, Style::new().fg(theme.accent)))
        .border_style(Style::new().fg(theme.accent));
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    // 搜索栏：作为循环导航的一环，选中时高亮
    let filter_sel_style = if p.on_filter {
        Style::new().fg(theme.accent).add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(theme.accent)
    };
    let filter_mark = if p.on_filter { "▸ " } else { "  " };
    let mut lines = vec![
        Line::from(vec![
            Span::styled(filter_mark, filter_sel_style),
            Span::styled("❯ ", filter_sel_style),
            Span::styled(p.filter.clone(), filter_sel_style),
        ]),
        Line::from(""),
    ];
    let n_visible = inner
        .height
        .saturating_sub(if app.palette_kind == PaletteKind::Models {
            3
        } else {
            2
        }) as usize;
    let start = if p.visible.len() > n_visible {
        p.selected
            .saturating_sub(n_visible / 2)
            .min(p.visible.len() - n_visible)
    } else {
        0
    };
    for (i, item) in p.visible.iter().skip(start).take(n_visible).enumerate() {
        let idx = start + i;
        let sel = idx == p.selected;
        let style = if sel {
            Style::new().fg(theme.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(theme.dim)
        };
        let mark = if sel { "▸ " } else { "  " };
        let star = if item.starred { "★ " } else { "  " };
        let hint = if item.hint.is_empty() {
            String::new()
        } else {
            format!("  {}", item.hint)
        };
        lines.push(Line::from(vec![
            Span::styled(mark, style),
            Span::styled(star, Style::new().fg(theme.accent)),
            Span::styled(item.label.clone(), style),
            Span::styled(hint, Style::new().fg(theme.faint)),
        ]));
    }
    if p.visible.is_empty() {
        lines.push(Line::from(Span::styled(
            "（无匹配）",
            Style::new().fg(theme.faint),
        )));
    }
    // 模型面板底部提示
    if app.palette_kind == PaletteKind::Models {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            " ↑↓ 导航（含搜索栏）· f 收藏模型 · Enter 选择 ",
            Style::new().fg(theme.faint),
        )));
    }
    f.render_widget(Paragraph::new(lines), inner);
}
