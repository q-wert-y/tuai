//! 布局辅助：居中矩形。

use ratatui::layout::{Constraint, Layout, Rect};

/// 按百分比居中的矩形（percent_x / percent_y 取 0-100）。
pub fn centered(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let pv = (100 - percent_y) / 2;
    let ph = (100 - percent_x) / 2;
    let v = Layout::vertical([
        Constraint::Percentage(pv),
        Constraint::Percentage(percent_y),
        Constraint::Percentage(pv),
    ])
    .split(area);
    let h = Layout::horizontal([
        Constraint::Percentage(ph),
        Constraint::Percentage(percent_x),
        Constraint::Percentage(ph),
    ])
    .split(v[1]);
    h[1]
}
