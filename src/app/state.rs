//! 事件与 UI 状态定义。

use crate::commands::Action;
use crate::model::{Message, Session};

/// 主循环事件。
#[derive(Debug)]
pub enum AppEvent {
    /// 终端事件（键盘 / 鼠标 / 尺寸变化）
    Term(crossterm::event::Event),
    /// 后台 LLM 流事件
    Llm(crate::llm::LlmEvent),
    /// 模型列表拉取结果
    Models(Result<Vec<String>, String>),
    /// 定时 tick（动画 / toast 过期）
    Tick,
}

/// 焦点区域。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Input,
    Chat,
    Sidebar,
}

impl Focus {
    pub fn next(self) -> Focus {
        match self {
            Focus::Input => Focus::Chat,
            Focus::Chat => Focus::Sidebar,
            Focus::Sidebar => Focus::Input,
        }
    }
}

/// 命令面板状态。
pub struct PaletteState {
    pub filter: String,
    pub selected: usize,
    /// 过滤后的可见条目
    pub visible: Vec<PaletteItem>,
    /// 全量条目
    pub items: Vec<PaletteItem>,
}

pub struct PaletteItem {
    pub label: String,
    pub hint: String,
    pub action: Action,
    /// 收藏标记（模型面板显示 ★）
    pub starred: bool,
}

impl PaletteState {
    pub fn commands() -> PaletteState {
        let items = crate::commands::all()
            .into_iter()
            .map(|c| PaletteItem {
                label: c.label.to_string(),
                hint: c.hint.to_string(),
                action: c.action,
                starred: false,
            })
            .collect();
        let mut p = PaletteState {
            filter: String::new(),
            selected: 0,
            visible: Vec::new(),
            items,
        };
        p.refilter();
        p
    }

    /// 模型面板：收藏的模型置顶并带 ★，其余去重后追加。
    pub fn models(models: Vec<String>, favorites: Vec<String>) -> PaletteState {
        let mut items: Vec<PaletteItem> = favorites
            .into_iter()
            .map(|m| PaletteItem {
                label: m.clone(),
                hint: String::new(),
                action: Action::SetModel(m),
                starred: true,
            })
            .collect();
        for m in models {
            if !items.iter().any(|i| i.label == m) {
                items.push(PaletteItem {
                    label: m.clone(),
                    hint: String::new(),
                    action: Action::SetModel(m),
                    starred: false,
                });
            }
        }
        let mut p = PaletteState {
            filter: String::new(),
            selected: 0,
            visible: Vec::new(),
            items,
        };
        p.refilter();
        p
    }

    pub fn providers(action: impl Fn(String) -> Action, names: Vec<String>) -> PaletteState {
        let items = names
            .into_iter()
            .map(|n| PaletteItem {
                label: n.clone(),
                hint: String::new(),
                action: action(n),
                starred: false,
            })
            .collect();
        let mut p = PaletteState {
            filter: String::new(),
            selected: 0,
            visible: Vec::new(),
            items,
        };
        p.refilter();
        p
    }

    /// 应用 fuzzy 过滤并重置选中项（保留当前选中 label 若仍可见）。
    /// 命令面板：label（中文描述）与 hint（斜杠命令）任一命中即收录。
    pub fn refilter(&mut self) {
        let keep = self.visible.get(self.selected).map(|i| i.label.clone());
        let needle = self.filter.as_str();
        self.visible = self
            .items
            .iter()
            .filter_map(|item| {
                crate::util::fuzzy_score(&item.label, needle)
                    .max(crate::util::fuzzy_score(&item.hint, needle))
                    .map(|_| item.clone_item())
            })
            .collect();
        self.selected = 0;
        if let Some(label) = keep {
            if let Some(idx) = self.visible.iter().position(|i| i.label == label) {
                self.selected = idx;
            }
        }
    }

    pub fn move_up(&mut self) {
        // 循环导航：顶端按上 → 跳到底端
        if !self.visible.is_empty() {
            self.selected = if self.selected == 0 {
                self.visible.len() - 1
            } else {
                self.selected - 1
            };
        }
    }

    pub fn move_down(&mut self) {
        // 循环导航：底端按下 → 跳到顶端
        if !self.visible.is_empty() {
            self.selected = (self.selected + 1) % self.visible.len();
        }
    }
}

impl PaletteItem {
    fn clone_item(&self) -> PaletteItem {
        PaletteItem {
            label: self.label.clone(),
            hint: self.hint.clone(),
            action: self.action.clone(),
            starred: self.starred,
        }
    }
}

/// 表单字段。
pub struct FormField {
    pub label: String,
    pub value: String,
    /// 光标（字符索引）
    pub cx: usize,
    /// 掩码显示（API key）
    pub secret: bool,
}

/// 表单用途。
#[derive(Debug, Clone, PartialEq)]
pub enum FormPurpose {
    ProviderAdd,
    ProviderEdit {
        name: String,
    },
    RenameSession {
        session_id: i64,
    },
    SystemPrompt {
        session_id: i64,
    },
    /// 编辑消息：regenerate = 编辑 user 消息后截断并重新生成
    EditMessage {
        message_id: i64,
        regenerate: bool,
    },
}

/// 表单状态（模态）。
pub struct FormState {
    pub title: String,
    pub purpose: FormPurpose,
    pub fields: Vec<FormField>,
    pub idx: usize,
    /// 提示信息
    pub hint: String,
}

impl FormState {
    pub fn provider(
        purpose: FormPurpose,
        name: &str,
        base_url: &str,
        api_key: &str,
        model: &str,
    ) -> FormState {
        FormState {
            title: match &purpose {
                FormPurpose::ProviderEdit { .. } => "编辑提供商".into(),
                _ => "添加提供商".into(),
            },
            purpose,
            fields: vec![
                FormField {
                    label: "名称".into(),
                    value: name.into(),
                    cx: name.chars().count(),
                    secret: false,
                },
                FormField {
                    label: "Base URL（以 /v1 结尾）".into(),
                    value: base_url.into(),
                    cx: base_url.chars().count(),
                    secret: false,
                },
                FormField {
                    label: "API Key（留空 = 不变）".into(),
                    value: api_key.into(),
                    cx: api_key.chars().count(),
                    secret: true,
                },
                FormField {
                    label: "默认模型".into(),
                    value: model.into(),
                    cx: model.chars().count(),
                    secret: false,
                },
            ],
            idx: 0,
            hint: "Tab 切换字段 · Enter 提交 · Esc 取消".into(),
        }
    }

    pub fn simple(title: &str, purpose: FormPurpose, label: &str, value: &str) -> FormState {
        FormState {
            title: title.to_string(),
            purpose,
            fields: vec![FormField {
                label: label.to_string(),
                value: value.to_string(),
                cx: value.chars().count(),
                secret: false,
            }],
            idx: 0,
            hint: "Enter 提交 · Esc 取消".into(),
        }
    }

    pub fn field(&self) -> &FormField {
        &self.fields[self.idx]
    }

    pub fn field_mut(&mut self) -> &mut FormField {
        &mut self.fields[self.idx]
    }

    pub fn next_field(&mut self) {
        self.idx = (self.idx + 1) % self.fields.len();
    }

    /// 插入字符到当前字段。
    pub fn insert_char(&mut self, c: char) {
        let f = &mut self.fields[self.idx];
        let byte_idx = char_to_byte(&f.value, f.cx);
        f.value.insert(byte_idx, c);
        f.cx += 1;
    }

    /// 换行（系统提示词 / 编辑消息支持多行）。
    pub fn newline(&mut self) {
        let f = &mut self.fields[self.idx];
        let byte_idx = char_to_byte(&f.value, f.cx);
        f.value.insert(byte_idx, '\n');
        f.cx += 1;
    }

    /// 在光标处插入整段文本（粘贴），保留换行并统一为 \n。
    pub fn insert_text(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let f = &mut self.fields[self.idx];
        let byte_idx = char_to_byte(&f.value, f.cx);
        f.value.insert_str(byte_idx, &text);
        f.cx += text.chars().count();
    }

    pub fn backspace(&mut self) {
        if self.fields[self.idx].cx > 0 {
            let f = &mut self.fields[self.idx];
            f.cx -= 1;
            let byte_idx = char_to_byte(&f.value, f.cx);
            f.value.remove(byte_idx);
        }
    }

    pub fn value(&self, i: usize) -> String {
        self.fields[i].value.trim().to_string()
    }
}

fn char_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

/// 多行输入框状态。
#[derive(Default)]
pub struct InputState {
    pub lines: Vec<String>,
    pub cy: usize,
    pub cx: usize,
    pub history: Vec<String>,
    pub hist_idx: Option<usize>,
}

impl InputState {
    pub fn new() -> InputState {
        InputState {
            lines: vec![String::new()],
            ..Default::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.lines.iter().all(|l| l.is_empty())
    }

    pub fn insert_char(&mut self, c: char) {
        let line = &mut self.lines[self.cy];
        let byte = char_to_byte(line, self.cx);
        line.insert(byte, c);
        self.cx += 1;
    }

    pub fn newline(&mut self) {
        let byte = char_to_byte(&self.lines[self.cy], self.cx);
        let rest = self.lines[self.cy].split_off(byte);
        self.lines.insert(self.cy + 1, rest);
        self.cy += 1;
        self.cx = 0;
    }

    pub fn backspace(&mut self) {
        if self.cx > 0 {
            let line = &mut self.lines[self.cy];
            self.cx -= 1;
            let byte = char_to_byte(line, self.cx);
            line.remove(byte);
        } else if self.cy > 0 {
            let cur = self.lines.remove(self.cy);
            self.cy -= 1;
            self.cx = self.lines[self.cy].chars().count();
            self.lines[self.cy].push_str(&cur);
        }
    }

    pub fn delete(&mut self) {
        let line_len = self.lines[self.cy].chars().count();
        if self.cx < line_len {
            let line = &mut self.lines[self.cy];
            let byte = char_to_byte(line, self.cx + 1);
            line.remove(byte);
        } else if self.cy + 1 < self.lines.len() {
            let next = self.lines.remove(self.cy + 1);
            self.lines[self.cy].push_str(&next);
        }
    }

    /// 在光标处插入整段文本（粘贴），保留换行并统一为 \n。
    /// 多行时按 \n 拆分插入，光标定位到插入文本末尾。
    pub fn insert_text(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        if !text.contains('\n') {
            let byte = char_to_byte(&self.lines[self.cy], self.cx);
            self.lines[self.cy].insert_str(byte, &text);
            self.cx += text.chars().count();
            return;
        }
        let byte = char_to_byte(&self.lines[self.cy], self.cx);
        let head = self.lines[self.cy][..byte].to_string();
        let tail = self.lines[self.cy][byte..].to_string();
        let mut parts: Vec<String> = text.split('\n').map(|s| s.to_string()).collect();
        let mut lines = self.lines[..self.cy].to_vec();
        lines.push(head + &parts.remove(0));
        lines.append(&mut parts);
        let last = lines.len() - 1;
        lines[last] = format!("{}{}", lines[last], tail);
        self.cy = last;
        self.cx = lines[last].chars().count();
        self.lines = lines;
    }

    pub fn left(&mut self) {
        if self.cx > 0 {
            self.cx -= 1;
        } else if self.cy > 0 {
            self.cy -= 1;
            self.cx = self.lines[self.cy].chars().count();
        }
    }

    pub fn right(&mut self) {
        if self.cx < self.lines[self.cy].chars().count() {
            self.cx += 1;
        } else if self.cy + 1 < self.lines.len() {
            self.cy += 1;
            self.cx = 0;
        }
    }

    pub fn home(&mut self) {
        self.cx = 0;
    }

    pub fn end(&mut self) {
        self.cx = self.lines[self.cy].chars().count();
    }

    /// 上移：多行内先走行，单行走历史。
    pub fn up(&mut self) {
        if self.cy > 0 {
            self.cy -= 1;
            self.cx = self.cx.min(self.lines[self.cy].chars().count());
        } else {
            self.history_prev();
        }
    }

    pub fn down(&mut self) {
        if self.cy + 1 < self.lines.len() {
            self.cy += 1;
            self.cx = self.cx.min(self.lines[self.cy].chars().count());
        } else {
            self.history_next();
        }
    }

    fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        let idx = match self.hist_idx {
            None => self.history.len() - 1,
            Some(i) => i.saturating_sub(1),
        };
        self.hist_idx = Some(idx);
        self.load_history(idx);
    }

    fn history_next(&mut self) {
        if let Some(i) = self.hist_idx {
            if i + 1 < self.history.len() {
                self.hist_idx = Some(i + 1);
                self.load_history(i + 1);
            } else {
                self.hist_idx = None;
                self.reset();
            }
        }
    }

    fn load_history(&mut self, idx: usize) {
        let text = self.history[idx].clone();
        self.set_text(&text);
    }

    pub fn set_text(&mut self, text: &str) {
        self.lines = text.split('\n').map(|s| s.to_string()).collect();
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.cy = self.lines.len() - 1;
        self.cx = self.lines[self.cy].chars().count();
    }

    /// 取出全部文本（含换行），并清空缓冲。
    pub fn take(&mut self) -> String {
        let text = self.lines.join("\n");
        self.reset();
        text
    }

    pub fn reset(&mut self) {
        self.lines = vec![String::new()];
        self.cx = 0;
        self.cy = 0;
        self.hist_idx = None;
    }
}

/// 进行中的生成任务。
pub struct Generation {
    pub session_id: i64,
    pub content: String,
    pub reasoning: String,
    pub handle: tokio::task::JoinHandle<()>,
}

/// 会话列表 + 当前会话消息的应用层数据视图。
pub struct ChatView {
    pub sessions: Vec<Session>,
    pub current: Option<usize>,
    pub messages: Vec<Message>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_matches_hint_and_label() {
        // 输入斜杠命令词（hint）可命中
        let mut p = PaletteState::commands();
        p.filter = "model".into();
        p.refilter();
        assert!(p.visible.iter().any(|i| i.hint == "/model"));
        // 输入中文描述（label）可命中
        let mut p = PaletteState::commands();
        p.filter = "新建".into();
        p.refilter();
        assert!(p.visible.iter().any(|i| i.hint == "/new"));
    }

    #[test]
    fn palette_wrap_navigation() {
        let p = PaletteState::models(vec!["a".into(), "b".into(), "c".into()], Vec::new());
        let mut p = p;
        // 底端按下 → 顶端
        p.selected = p.visible.len() - 1;
        p.move_down();
        assert_eq!(p.selected, 0);
        // 顶端按上 → 底端
        p.move_up();
        assert_eq!(p.selected, p.visible.len() - 1);
    }

    #[test]
    fn palette_models_favorites_first() {
        let p = PaletteState::models(
            vec!["m1".into(), "m2".into()],
            vec!["fav".into(), "m1".into()],
        );
        assert_eq!(p.items[0].label, "fav");
        assert!(p.items[0].starred);
        // 收藏项去重：m1 只出现一次
        assert_eq!(p.items.iter().filter(|i| i.label == "m1").count(), 1);
    }

    #[test]
    fn input_insert_text_single() {
        let mut i = InputState::new();
        i.set_text("abc");
        i.cx = 1; // 光标在 'b' 前
        i.insert_text("XY");
        assert_eq!(i.take(), "aXYbc");
    }

    #[test]
    fn input_insert_text_multiline() {
        let mut i = InputState::new();
        i.insert_char('a');
        i.insert_char('b');
        i.insert_text("x\ny\nz");
        assert_eq!(i.take(), "abx\ny\nz");
    }

    #[test]
    fn input_insert_text_crlf() {
        let mut i = InputState::new();
        i.insert_text("a\r\nb");
        assert_eq!(i.take(), "a\nb");
    }
}
