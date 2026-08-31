//! 会话侧栏渲染。

use crate::app::{App, Focus};
use crate::util::truncate_by_width;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState};
use ratatui::Frame;

const SELECT_BG: Color = Color::Rgb(46, 52, 64);

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let theme = &app.theme;
    let focused = app.focus == Focus::Sidebar;
    let border = if focused {
        Style::new().fg(theme.accent)
    } else {
        Style::new().fg(theme.faint)
    };
    let block = Block::bordered()
        .title(Span::styled(" 会话 ", Style::new().fg(theme.dim)))
        .border_style(border);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let n = app.view.sessions.len();
    let items: Vec<ListItem> = app
        .view
        .sessions
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let is_cur = app.view.current == Some(i);
            let sym = if is_cur { "● " } else { "○ " };
            let title = truncate_by_width(&s.title, inner.width.saturating_sub(4) as usize);
            let style = if is_cur {
                Style::new().fg(theme.user).add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(theme.dim)
            };
            ListItem::new(Line::from(Span::styled(format!("{sym}{title}"), style)))
        })
        .collect();

    let mut list = List::new(items);
    if focused {
        list = list
            .highlight_style(Style::new().bg(SELECT_BG).fg(theme.accent))
            .highlight_symbol("▸ ");
    }
    let mut state = ListState::default();
    if n > 0 {
        state.select(Some(app.sidebar_sel.min(n - 1)));
    }
    f.render_stateful_widget(list, inner, &mut state);
}
