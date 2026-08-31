//! TUI 渲染总入口：布局分发与覆盖层（面板 / 表单 / 帮助）。

pub mod chat;
pub mod form;
pub mod input;
pub mod layout;
pub mod markdown;
pub mod palette;
pub mod sidebar;
pub mod status;
pub mod theme;

use crate::app::App;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use ratatui::Frame;

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let cols = Layout::horizontal([Constraint::Length(26), Constraint::Min(30)]).split(area);
    let main = cols[1];
    let input_h = input::height(app, main.width);
    let rows = Layout::vertical([
        Constraint::Min(6),
        Constraint::Length(input_h),
        Constraint::Length(1),
    ])
    .split(main);

    sidebar::render(f, cols[0], app);
    chat::render(f, rows[0], app);
    let mut cursor = input::render(f, rows[1], app);
    status::render(f, rows[2], app);

    // 覆盖层（优先级：面板 < 表单 < 帮助）
    if app.palette.is_some() {
        palette::render(f, area, app);
        cursor = None;
    }
    if app.form.is_some() {
        cursor = form::render(f, area, app);
    }
    if app.show_help {
        render_help(f, area, app);
        cursor = None;
    }
    if let Some((x, y)) = cursor {
        f.set_cursor_position((x, y));
    }
}

/// 帮助覆盖层。
fn render_help(f: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
    let rect = layout::centered(60, 60, area);
    f.render_widget(Clear, rect);
    let block = Block::bordered()
        .title(Span::styled(" 帮助 ", Style::new().fg(theme.accent)))
        .border_style(Style::new().fg(theme.accent));
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let rows: &[(&str, &str)] = &[
        ("Tab", "切换焦点：输入 ↔ 消息 ↔ 侧栏"),
        ("Enter", "发送 · Shift/Alt+Enter 换行"),
        ("Esc", "停止生成 / 关闭面板 / 返回输入"),
        ("j k ↑ ↓", "选择消息 / 列表导航（首尾循环）"),
        ("g / G", "跳首条 / 末条消息"),
        ("e", "编辑选中消息（消息区）"),
        ("dd", "删除选中消息（按两次 d）"),
        ("c", "复制最近代码块（消息区）"),
        ("n / r / d", "新建 / 重命名 / 删除会话（侧栏，d 按两次）"),
        ("f", "收藏模型（模型面板）或 /fav 当前模型"),
        ("Ctrl+R", "重新生成回复"),
        ("Ctrl+C ×2 / Ctrl+D", "退出"),
        ("/", "命令面板 · ? 本帮助"),
    ];
    let mut lines = Vec::new();
    for (k, v) in rows {
        lines.push(Line::from(vec![
            Span::styled(format!("{k:<20}"), Style::new().fg(theme.accent)),
            Span::styled(*v, Style::new().fg(theme.dim)),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "按 Esc 或 ? 关闭",
        Style::new().fg(theme.faint).add_modifier(Modifier::ITALIC),
    )));
    f.render_widget(Paragraph::new(lines), inner);
}
