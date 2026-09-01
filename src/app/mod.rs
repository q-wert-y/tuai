//! App 状态机与主事件循环：终端事件、LLM 流事件、命令分发与业务逻辑。

pub mod state;

pub use state::{
    AppEvent, ChatView, Focus, FormPurpose, FormState, Generation, InputState, PaletteState,
};

use crate::commands::{self, Action};
use crate::config::{Config, Provider};
use crate::llm::{ChatMessage, LlmEvent, OpenAiClient};
use crate::model::{Role, Session};
use crate::store::Store;
use crate::tui::theme::Theme;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// 命令面板用途。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteKind {
    Commands,
    Models,
    ProvidersEdit,
    ProvidersDelete,
    ProvidersSwitch,
}

/// 轻提示（3 秒过期）。
pub struct Toast {
    pub text: String,
    pub error: bool,
    pub at: Instant,
}

/// 应用状态机。
pub struct App {
    pub config: Config,
    pub store: Store,
    pub data_dir: PathBuf,
    pub theme: Theme,
    pub view: ChatView,
    pub focus: Focus,
    pub input: InputState,
    /// None = 跟随底部；Some = 固定滚动偏移
    pub chat_scroll: Option<u16>,
    /// 渲染时记录的聊天区行数 / 高度（供滚动计算）
    pub chat_total: u16,
    pub chat_height: u16,
    pub sidebar_sel: usize,
    pub delete_confirm: bool,
    /// 消息区选中消息（ChatView.messages 的索引）
    pub chat_sel: Option<usize>,
    /// 每条消息在聊天区渲染的行范围（start, end-exclusive），供自动滚动
    pub chat_msg_ranges: Vec<(u16, u16)>,
    /// 消息删除二次确认（dd）
    pub msg_delete_confirm: bool,
    pub generating: Option<Generation>,
    /// 生成期间的 type-ahead 队列
    pub queue: VecDeque<String>,
    pub palette: Option<PaletteState>,
    pub palette_kind: PaletteKind,
    pub form: Option<FormState>,
    pub show_help: bool,
    pub toast: Option<Toast>,
    pub ctrlc_at: Option<Instant>,
    pub models_loading: bool,
    pub quitting: bool,
    /// 普通模式收到 Esc 的待定时刻（等待判断是否为粘贴开始标记）
    pub esc_at: Option<Instant>,
    /// 开始标记 [200~ 已匹配的部分字符
    pub start_probe: Option<Vec<char>>,
    /// 正在收集的 bracketed paste 内容（Windows 下 crossterm 无 Paste 事件时兜底）
    paste: Option<PasteCollect>,
}

/// bracketed paste 序列的按键流重组器。
/// Windows 上 crossterm 走 ReadConsoleInputW，把 \x1b[200~..\x1b[201~ 拆成单个按键
/// （换行变成 Enter），不产生 Event::Paste —— 这里在按键流里重组。
struct PasteCollect {
    buf: Vec<u8>,
    esc: bool,
    /// -1 = 无探测；0..4 = 正在匹配 "[201~" 的 '[' 之后部分
    probe: i32,
    done: bool,
}

impl PasteCollect {
    fn new() -> Self {
        PasteCollect {
            buf: Vec::new(),
            esc: false,
            probe: -1,
            done: false,
        }
    }

    fn feed(&mut self, key: KeyEvent) {
        if self.probe >= 0 {
            if let KeyCode::Char(c) = key.code {
                let expect = "201~".as_bytes()[self.probe as usize] as char;
                if c == expect {
                    self.probe += 1;
                    if self.probe == 4 {
                        self.done = true;
                    }
                    return;
                }
            }
            // 探测失败：把挂起的 ESC 与 '[' 作为内容保留
            self.buf.push(b'\x1b');
            self.buf.push(b'[');
            self.esc = false;
            self.probe = -1;
        }
        match key.code {
            KeyCode::Esc => {
                if self.esc {
                    // 连续两个 ESC：内容中的转义字符
                    self.buf.push(b'\x1b');
                    self.esc = false;
                } else {
                    self.esc = true;
                }
            }
            KeyCode::Char('[') if self.esc => {
                // 候选结束标记 \x1b[201~
                self.probe = 0;
                self.esc = false;
            }
            KeyCode::Char(c) => {
                let mut tmp = [0u8; 4];
                self.buf
                    .extend_from_slice(c.encode_utf8(&mut tmp).as_bytes());
                self.esc = false;
            }
            KeyCode::Enter => {
                // 粘贴内容的换行被 conhost 转成 Enter 键
                self.buf.push(b'\n');
                self.esc = false;
            }
            _ => {}
        }
    }

    fn finish(self) -> String {
        String::from_utf8_lossy(&self.buf).into_owned()
    }
}

impl App {
    pub fn new(config: Config, store: Store, data_dir: PathBuf) -> anyhow::Result<App> {
        let mut sessions = store.list_sessions()?;
        // 首次运行：无会话则自动创建（沿用上次使用的提供商/模型，否则选可用的）
        if sessions.is_empty() {
            let (provider, model) = new_session_provider_model(&config);
            let s = store.create_session(Session::DEFAULT_TITLE, &provider, &model, None)?;
            sessions.insert(0, s);
        }
        let messages = store.messages(sessions[0].id)?;
        let mut app = App {
            view: ChatView {
                sessions,
                current: Some(0),
                messages,
            },
            config,
            store,
            data_dir,
            theme: Theme::default(),
            focus: Focus::Input,
            input: InputState::new(),
            chat_scroll: None,
            chat_total: 0,
            chat_height: 1,
            sidebar_sel: 0,
            delete_confirm: false,
            chat_sel: None,
            chat_msg_ranges: Vec::new(),
            msg_delete_confirm: false,
            generating: None,
            queue: VecDeque::new(),
            palette: None,
            palette_kind: PaletteKind::Commands,
            form: None,
            show_help: false,
            toast: None,
            ctrlc_at: None,
            models_loading: false,
            quitting: false,
            esc_at: None,
            start_probe: None,
            paste: None,
        };
        // 无任何提供商（首次运行）：直接打开添加表单引导配置
        if app.config.providers.is_empty() {
            app.form = Some(FormState::provider(
                FormPurpose::ProviderAdd,
                "",
                "",
                "",
                "",
            ));
        }
        Ok(app)
    }

    /// 当前会话。
    pub fn current_session(&self) -> Option<&Session> {
        self.view.current.and_then(|i| self.view.sessions.get(i))
    }

    /// 当前会话索引（可变）。
    fn cur_idx(&self) -> Option<usize> {
        self.view.current
    }

    fn toast(&mut self, text: impl Into<String>, error: bool) {
        self.toast = Some(Toast {
            text: text.into(),
            error,
            at: Instant::now(),
        });
    }

    // ---------- 事件分发 ----------

    pub fn dispatch(&mut self, ev: AppEvent, tx: &mpsc::Sender<AppEvent>) {
        match ev {
            AppEvent::Term(e) => self.on_term(e, tx),
            AppEvent::Llm(e) => self.on_llm(e, tx),
            AppEvent::Models(r) => self.on_models(r),
            AppEvent::Tick => self.on_tick(tx),
        }
    }

    fn on_term(&mut self, e: Event, tx: &mpsc::Sender<AppEvent>) {
        match e {
            Event::Key(key) if key.kind == KeyEventKind::Press => self.on_key(key, tx),
            Event::Mouse(m) if self.palette.is_none() && self.form.is_none() => match m.kind {
                MouseEventKind::ScrollUp => self.scroll_up(3),
                MouseEventKind::ScrollDown => self.scroll_down(3),
                _ => {}
            },
            Event::Paste(text) => self.on_paste(&text),
            _ => {}
        }
    }

    /// 处理终端粘贴事件（bracketed paste）：按当前界面状态插入文本。
    fn on_paste(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if self.form.is_some() {
            if let Some(f) = &mut self.form {
                f.insert_text(text);
            }
            return;
        }
        if let Some(p) = &mut self.palette {
            // 命令面板过滤框：逐字符插入（忽略换行）
            for c in text.chars() {
                if c == '\n' || c == '\r' {
                    continue;
                }
                p.filter.push(c);
            }
            p.refilter();
            return;
        }
        // 默认粘贴到输入框（保留换行）
        self.focus = Focus::Input;
        self.input.insert_text(text);
    }

    fn on_key(&mut self, key: KeyEvent, tx: &mpsc::Sender<AppEvent>) {
        // 手动重组 bracketed paste（crossterm 在 Windows 上不产生 Event::Paste）
        if self.feed_paste(key, tx) {
            return;
        }
        // 全局：Ctrl 组合键
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            if let KeyCode::Char(c) = key.code {
                match c {
                    'c' => {
                        let quit = self
                            .ctrlc_at
                            .map(|t| t.elapsed() < Duration::from_millis(1500))
                            .unwrap_or(false);
                        if quit {
                            self.quitting = true;
                        } else {
                            self.ctrlc_at = Some(Instant::now());
                            self.toast("再按一次 Ctrl+C 退出", false);
                        }
                        return;
                    }
                    'd' => {
                        self.quitting = true;
                        return;
                    }
                    'v' => {
                        self.paste_clipboard();
                        return;
                    }
                    'r' => {
                        self.regenerate(tx);
                        return;
                    }
                    _ => return,
                }
            }
            return;
        }
        // 模态层优先
        if self.form.is_some() {
            self.on_form_key(key, tx);
            return;
        }
        if self.palette.is_some() {
            self.on_palette_key(key, tx);
            return;
        }
        if self.show_help {
            match key.code {
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => self.show_help = false,
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Tab => {
                self.focus = self.focus.next();
                self.delete_confirm = false;
                self.msg_delete_confirm = false;
                if self.focus == Focus::Sidebar {
                    self.sidebar_sel = self.view.current.unwrap_or(0);
                } else if self.focus == Focus::Chat {
                    // 进入消息区：默认选中最后一条消息
                    self.chat_sel = if self.view.messages.is_empty() {
                        None
                    } else {
                        Some(self.view.messages.len() - 1)
                    };
                }
                return;
            }
            KeyCode::Esc => {
                self.do_esc(tx);
                return;
            }
            _ => {}
        }
        match self.focus {
            Focus::Input => self.on_input_key(key, tx),
            Focus::Chat => self.on_chat_key(key, tx),
            Focus::Sidebar => self.on_sidebar_key(key),
        }
    }

    fn on_input_key(&mut self, key: KeyEvent, tx: &mpsc::Sender<AppEvent>) {
        match key.code {
            KeyCode::Enter
                if key
                    .modifiers
                    .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) =>
            {
                self.input.newline();
            }
            KeyCode::Enter => self.submit_input(tx),
            KeyCode::Char(c) => {
                if key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                {
                    return;
                }
                // 空输入时：'/' 打开命令面板、'?' 切换帮助
                if self.input.is_empty() {
                    if c == '/' {
                        self.palette = Some(PaletteState::commands());
                        self.palette_kind = PaletteKind::Commands;
                        return;
                    }
                    if c == '?' {
                        self.show_help = true;
                        return;
                    }
                }
                self.input.insert_char(c);
            }
            KeyCode::Backspace => self.input.backspace(),
            KeyCode::Delete => self.input.delete(),
            KeyCode::Left => self.input.left(),
            KeyCode::Right => self.input.right(),
            KeyCode::Home => self.input.home(),
            KeyCode::End => self.input.end(),
            KeyCode::Up => self.input.up(),
            KeyCode::Down => self.input.down(),
            _ => {}
        }
    }

    fn submit_input(&mut self, tx: &mpsc::Sender<AppEvent>) {
        if self.input.is_empty() {
            return;
        }
        let text = self.input.take();
        // 单行且以 '/' 开头 → 斜杠命令
        if text.starts_with('/') && !text.contains('\n') {
            match commands::parse_slash(&text) {
                Some(action) => self.execute_action(action, tx),
                None => self.toast(format!("未知命令 {}（? 查看帮助）", text.trim()), true),
            }
            return;
        }
        self.input.history.push(text.clone());
        self.send_message(&text, tx);
    }

    fn on_chat_key(&mut self, key: KeyEvent, _tx: &mpsc::Sender<AppEvent>) {
        let is_d = key.code == KeyCode::Char('d');
        if !is_d {
            self.msg_delete_confirm = false;
        }
        let n = self.view.messages.len();
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                let next = match self.chat_sel {
                    None => Some(0),
                    Some(i) if n > 0 => Some((i + 1) % n), // 循环：末条按下 → 首条
                    Some(i) => Some(i),
                };
                self.sel_message(next);
            }
            KeyCode::Char('k') | KeyCode::Up => self.sel_message(match self.chat_sel {
                None if n > 0 => Some(n - 1),
                None => None,
                Some(0) if n > 0 => Some(n - 1), // 循环：首条按上 → 末条
                Some(i) => Some(i - 1),
            }),
            KeyCode::Char('g') => {
                if n > 0 {
                    self.sel_message(Some(0));
                }
            }
            KeyCode::Char('G') => {
                if n > 0 {
                    self.chat_sel = Some(n - 1);
                    self.chat_scroll = None;
                }
            }
            KeyCode::Char('c') => self.copy_code(),
            KeyCode::Char('e') => self.edit_selected_message(),
            KeyCode::Char('d') => {
                if self.msg_delete_confirm {
                    self.msg_delete_confirm = false;
                    self.delete_selected_message();
                } else {
                    self.msg_delete_confirm = true;
                    self.toast("再按一次 d 删除选中消息", false);
                }
            }
            _ => {}
        }
    }

    /// 设置选中的消息并滚动到可见位置。
    fn sel_message(&mut self, i: Option<usize>) {
        self.chat_sel = i;
        self.scroll_to_sel();
    }

    /// 滚动聊天区，使选中消息可见。
    fn scroll_to_sel(&mut self) {
        let Some(i) = self.chat_sel else { return };
        let Some(&(start, end)) = self.chat_msg_ranges.get(i) else {
            self.chat_scroll = None;
            return;
        };
        let h = self.chat_height;
        let max = self.chat_total.saturating_sub(h);
        let cur = self.chat_scroll.unwrap_or(max);
        let mut o = cur;
        if start < o {
            o = start; // 消息头在可视区上方 → 上滚
        }
        if end > o + h {
            o = (end).saturating_sub(h); // 消息尾在可视区下方 → 下滚
        }
        o = o.min(max);
        self.chat_scroll = if o >= max { None } else { Some(o) };
    }

    /// 编辑选中消息：user 消息提交后截断重发；assistant 仅改内容。
    fn edit_selected_message(&mut self) {
        if self.generating.is_some() {
            self.toast("生成中，稍后再编辑", false);
            return;
        }
        let Some(i) = self.chat_sel else { return };
        let Some(m) = self.view.messages.get(i) else {
            return;
        };
        let (id, is_user, content) = (m.id, m.role == Role::User, m.content.clone());
        self.form = Some(FormState::simple(
            if is_user {
                "编辑消息（提交后重新生成回复）"
            } else {
                "编辑消息"
            },
            FormPurpose::EditMessage {
                message_id: id,
                regenerate: is_user,
            },
            "内容（Shift+Enter 换行）",
            &content,
        ));
    }

    /// 删除选中消息（单条）。
    fn delete_selected_message(&mut self) {
        let Some(i) = self.chat_sel else { return };
        let Some(m) = self.view.messages.get(i) else {
            return;
        };
        let (id, sid) = (m.id, m.session_id);
        if let Err(e) = self.store.delete_message(id) {
            self.toast(format!("删除消息失败: {e}"), true);
            return;
        }
        match self.store.messages(sid) {
            Ok(msgs) => self.view.messages = msgs,
            Err(e) => {
                self.toast(format!("重载消息失败: {e}"), true);
                return;
            }
        }
        let n = self.view.messages.len();
        self.chat_sel = if n == 0 { None } else { Some(i.min(n - 1)) };
        self.chat_scroll = None;
        self.toast("已删除消息", false);
    }

    fn on_sidebar_key(&mut self, key: KeyEvent) {
        let is_d = key.code == KeyCode::Char('d');
        if !is_d {
            self.delete_confirm = false;
        }
        let n = self.view.sessions.len();
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                if n > 0 {
                    // 循环：末位按下 → 首位
                    self.sidebar_sel = (self.sidebar_sel + 1) % n;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if n > 0 {
                    // 循环：首位按上 → 末位
                    self.sidebar_sel = if self.sidebar_sel == 0 {
                        n - 1
                    } else {
                        self.sidebar_sel - 1
                    };
                }
            }
            KeyCode::Enter => self.select_session(self.sidebar_sel),
            KeyCode::Char('n') => self.new_session(),
            KeyCode::Char('r') => {
                // 重命名选中会话
                if let Some(s) = self.view.sessions.get(self.sidebar_sel) {
                    let (id, title) = (s.id, s.title.clone());
                    self.form = Some(FormState::simple(
                        "重命名会话",
                        FormPurpose::RenameSession { session_id: id },
                        "新标题",
                        &title,
                    ));
                }
            }
            KeyCode::Char('d') => {
                if self.delete_confirm {
                    self.delete_confirm = false;
                    self.delete_selected_session();
                } else {
                    self.delete_confirm = true;
                    self.toast("再按一次 d 确认删除选中会话", false);
                }
            }
            _ => {}
        }
    }

    // ---------- 面板 ----------

    fn on_palette_key(&mut self, key: KeyEvent, tx: &mpsc::Sender<AppEvent>) {
        match key.code {
            KeyCode::Esc => self.palette = None,
            KeyCode::Up => {
                if let Some(p) = &mut self.palette {
                    p.move_up();
                }
            }
            KeyCode::Down => {
                if let Some(p) = &mut self.palette {
                    p.move_down();
                }
            }
            KeyCode::Backspace => {
                if let Some(p) = &mut self.palette {
                    p.filter.pop();
                    p.refilter();
                }
            }
            KeyCode::Char(c) => {
                if key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                {
                    return;
                }
                // 模型面板：f 收藏/取消选中模型
                if c == 'f' && self.palette_kind == PaletteKind::Models {
                    self.toggle_palette_favorite();
                    return;
                }
                if let Some(p) = &mut self.palette {
                    p.filter.push(c);
                    p.refilter();
                }
            }
            KeyCode::Enter => {
                let selected = self.palette.as_ref().and_then(|p| {
                    p.visible
                        .get(p.selected)
                        .map(|i| (i.label.clone(), i.action.clone()))
                });
                self.palette = None;
                let Some((label, action)) = selected else {
                    return;
                };
                match self.palette_kind {
                    PaletteKind::Commands => {
                        self.execute_action(action, tx);
                    }
                    PaletteKind::Models => {
                        self.execute_action(Action::SetModel(label), tx);
                    }
                    PaletteKind::ProvidersEdit => self.open_provider_edit_form(&label),
                    PaletteKind::ProvidersDelete => {
                        self.execute_action(Action::RemoveProvider(label), tx);
                    }
                    PaletteKind::ProvidersSwitch => {
                        self.execute_action(Action::SwitchProvider(label), tx);
                    }
                }
            }
            _ => {}
        }
    }

    // ---------- 表单 ----------

    fn on_form_key(&mut self, key: KeyEvent, tx: &mpsc::Sender<AppEvent>) {
        match key.code {
            KeyCode::Esc => self.form = None,
            KeyCode::Tab => {
                if let Some(f) = &mut self.form {
                    f.next_field();
                }
            }
            KeyCode::Enter
                if key
                    .modifiers
                    .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT)
                    && self
                        .form
                        .as_ref()
                        .map(|f| f.fields.len() == 1)
                        .unwrap_or(false) =>
            {
                // 单字段表单（系统提示词/编辑消息）支持多行
                if let Some(f) = &mut self.form {
                    f.newline();
                }
            }
            KeyCode::Backspace => {
                if let Some(f) = &mut self.form {
                    f.backspace();
                }
            }
            KeyCode::Left => {
                if let Some(f) = &mut self.form {
                    if f.field().cx > 0 {
                        f.field_mut().cx -= 1;
                    }
                }
            }
            KeyCode::Right => {
                if let Some(f) = &mut self.form {
                    let len = f.field().value.chars().count();
                    f.field_mut().cx = (f.field_mut().cx + 1).min(len);
                }
            }
            KeyCode::Enter => self.submit_form(tx),
            KeyCode::Char(c) => {
                if key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                {
                    return;
                }
                if let Some(f) = &mut self.form {
                    f.insert_char(c);
                }
            }
            _ => {}
        }
    }

    fn submit_form(&mut self, tx: &mpsc::Sender<AppEvent>) {
        let Some(form) = self.form.take() else { return };
        // 校验
        let bad = match &form.purpose {
            FormPurpose::ProviderAdd | FormPurpose::ProviderEdit { .. } => {
                form.value(0).is_empty() || form.value(1).is_empty()
            }
            _ => false,
        };
        if bad {
            self.toast("名称与 Base URL 不能为空", true);
            self.form = Some(form);
            return;
        }
        match &form.purpose {
            FormPurpose::ProviderAdd => {
                let name = form.value(0);
                if self.config.providers.iter().any(|p| p.name == name) {
                    self.toast(format!("提供商 {name} 已存在"), true);
                    self.form = Some(form);
                    return;
                }
                let model = form.value(3);
                let model = if model.is_empty() {
                    "gpt-4o-mini".to_string()
                } else {
                    model
                };
                let is_first = self.config.providers.is_empty();
                self.config.providers.push(Provider {
                    name: name.clone(),
                    base_url: form.value(1),
                    api_key: form.value(2),
                    default_model: model.clone(),
                });
                if is_first {
                    // 首个提供商：设为默认并同步当前会话
                    self.config.default_provider = name.clone();
                    if let Some(idx) = self.cur_idx() {
                        let id = self.view.sessions[idx].id;
                        let _ = self.store.update_session_provider(id, &name);
                        let _ = self.store.update_session_model(id, &model);
                        self.view.sessions[idx].provider = name.clone();
                        self.view.sessions[idx].model = model.clone();
                    }
                }
                self.save_config_or_toast();
                self.toast(format!("已添加提供商 {name}"), false);
            }
            FormPurpose::ProviderEdit { name } => {
                let orig = name.clone();
                let new_name = form.value(0);
                let Some(p) = self.config.providers.iter_mut().find(|p| p.name == orig) else {
                    return;
                };
                p.base_url = form.value(1);
                let key = form.value(2);
                if !key.is_empty() {
                    p.api_key = key;
                }
                let model = form.value(3);
                if !model.is_empty() {
                    p.default_model = model;
                }
                if !new_name.is_empty() && new_name != orig {
                    p.name = new_name.clone();
                    if self.config.default_provider == orig {
                        self.config.default_provider = new_name.clone();
                    }
                    if let Err(e) = self.store.rename_session_provider(&orig, &new_name) {
                        self.toast(format!("同步会话引用失败: {e}"), true);
                    }
                    for s in &mut self.view.sessions {
                        if s.provider == orig {
                            s.provider = new_name.clone();
                        }
                    }
                }
                self.save_config_or_toast();
                self.toast("提供商已更新", false);
            }
            FormPurpose::RenameSession { session_id } => {
                let session_id = *session_id;
                let title = form.value(0);
                if title.is_empty() {
                    self.toast("标题不能为空", true);
                    self.form = Some(form);
                    return;
                }
                match self.store.update_session_title(session_id, &title) {
                    Ok(_) => {
                        if let Some(idx) = self.cur_idx() {
                            if self.view.sessions.get(idx).map(|s| s.id) == Some(session_id) {
                                self.view.sessions[idx].title = title.clone();
                            }
                        }
                        self.toast("会话已重命名", false);
                    }
                    Err(e) => self.toast(format!("重命名失败: {e}"), true),
                }
            }
            FormPurpose::SystemPrompt { session_id } => {
                let session_id = *session_id;
                let v = form.fields[0].value.trim().to_string();
                let opt = if v.is_empty() { None } else { Some(v) };
                match self
                    .store
                    .update_session_system_prompt(session_id, opt.as_deref())
                {
                    Ok(_) => {
                        if let Some(idx) = self.cur_idx() {
                            if self.view.sessions.get(idx).map(|s| s.id) == Some(session_id) {
                                self.view.sessions[idx].system_prompt = opt;
                            }
                        }
                        self.toast("系统提示词已更新", false);
                    }
                    Err(e) => self.toast(format!("更新失败: {e}"), true),
                }
            }
            FormPurpose::EditMessage {
                message_id,
                regenerate,
            } => {
                let (message_id, regenerate) = (*message_id, *regenerate);
                let content = form.fields[0].value.trim().to_string();
                if content.is_empty() {
                    self.toast("内容不能为空", true);
                    self.form = Some(form);
                    return;
                }
                if let Err(e) = self.store.update_message_content(message_id, &content) {
                    self.toast(format!("更新消息失败: {e}"), true);
                    return;
                }
                let Some(session_id) = self.current_session().map(|s| s.id) else {
                    return;
                };
                if regenerate {
                    // 编辑 user 消息：截断其后的消息并重新生成
                    if let Err(e) = self.store.delete_messages_after(session_id, message_id) {
                        self.toast(format!("截断消息失败: {e}"), true);
                        return;
                    }
                }
                let sel = self.chat_sel;
                match self.store.messages(session_id) {
                    Ok(m) => {
                        self.view.messages = m;
                        let n = self.view.messages.len();
                        self.chat_sel = sel.map(|i| i.min(n.saturating_sub(1))).filter(|_| n > 0);
                        self.chat_scroll = None;
                    }
                    Err(e) => self.toast(format!("重载消息失败: {e}"), true),
                }
                self.toast("消息已更新", false);
                if regenerate {
                    self.dispatch_generation(tx);
                }
            }
        }
    }

    // ---------- 消息与生成 ----------

    pub fn send_message(&mut self, text: &str, tx: &mpsc::Sender<AppEvent>) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let Some(idx) = self.cur_idx() else { return };
        let Some(session) = self.view.sessions.get(idx) else {
            return;
        };
        let session_id = session.id;
        let msg = match self
            .store
            .insert_message(session_id, Role::User, text, None)
        {
            Ok(m) => m,
            Err(e) => {
                self.toast(format!("保存消息失败: {e}"), true);
                return;
            }
        };
        self.view.messages.push(msg);
        // 标题自动生成：首条用户消息前 24 列
        if self.view.sessions[idx].title == Session::DEFAULT_TITLE {
            let title = crate::util::truncate_by_width(text, 24);
            if !title.is_empty() {
                match self.store.update_session_title(session_id, &title) {
                    Ok(_) => self.view.sessions[idx].title = title,
                    Err(e) => tracing::warn!(error = %e, "自动标题失败"),
                }
            }
        }
        self.chat_scroll = None;
        if self.generating.is_some() {
            // type-ahead：排队
            self.queue.push_back(text.to_string());
            self.toast("生成中，消息已加入队列", false);
        } else {
            self.dispatch_generation(tx);
        }
    }

    /// 发起一次流式生成（请求体仅 {model, messages, stream}）。
    fn dispatch_generation(&mut self, tx: &mpsc::Sender<AppEvent>) {
        if self.generating.is_some() {
            return;
        }
        let Some(idx) = self.cur_idx() else { return };
        let Some(session) = self.view.sessions.get(idx).cloned() else {
            return;
        };
        let Some(provider) = self.config.effective_provider(&session.provider) else {
            self.toast(
                format!(
                    "提供商 \"{}\" 不存在 · /provider use 切换",
                    session.provider
                ),
                true,
            );
            return;
        };
        let is_local =
            provider.base_url.contains("localhost") || provider.base_url.contains("127.0.0.1");
        if provider.api_key.trim().is_empty() && !is_local {
            self.toast(
                format!(
                    "提供商 \"{}\" 的 API Key 为空 · /provider edit 填写或 /provider use 切换",
                    session.provider
                ),
                true,
            );
            return;
        }
        // 历史 = 当前会话全部消息（role + content）
        let history: Vec<ChatMessage> = self
            .view
            .messages
            .iter()
            .filter(|m| m.role != Role::System)
            .map(|m| ChatMessage::new(m.role.as_str(), m.content.clone()))
            .collect();
        let system = session
            .system_prompt
            .clone()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                self.config
                    .system_prompt
                    .clone()
                    .filter(|s| !s.trim().is_empty())
            });

        let client = OpenAiClient::new(&provider, &session.model, self.config.proxy.as_deref());
        let (llm_tx, mut llm_rx) = mpsc::channel::<LlmEvent>(256);
        // 流式任务（失败以 Failed 事件表达）
        let handle = tokio::spawn(async move {
            client.chat_stream(system, &history, &llm_tx).await;
        });
        // 转发：LlmEvent → AppEvent::Llm
        let tx = tx.clone();
        tokio::spawn(async move {
            while let Some(e) = llm_rx.recv().await {
                if tx.send(AppEvent::Llm(e)).await.is_err() {
                    break;
                }
            }
        });
        self.generating = Some(Generation {
            session_id: session.id,
            content: String::new(),
            reasoning: String::new(),
            handle,
        });
        self.chat_scroll = None;
    }

    fn on_llm(&mut self, ev: LlmEvent, tx: &mpsc::Sender<AppEvent>) {
        if self.generating.is_none() {
            return; // 过期事件
        }
        match ev {
            LlmEvent::Delta { text } => {
                if let Some(g) = &mut self.generating {
                    g.content.push_str(&text);
                }
            }
            LlmEvent::Reasoning { text } => {
                if let Some(g) = &mut self.generating {
                    g.reasoning.push_str(&text);
                }
            }
            LlmEvent::Done => self.finalize_generation(None, tx),
            LlmEvent::Failed { error } => self.finalize_generation(Some(error), tx),
        }
    }

    /// 收尾：持久化已生成内容，成功/停止后续发队列。
    fn finalize_generation(&mut self, error: Option<String>, tx: &mpsc::Sender<AppEvent>) {
        let Some(gen) = self.generating.take() else {
            return;
        };
        let reasoning = if gen.reasoning.is_empty() {
            None
        } else {
            Some(gen.reasoning.as_str())
        };
        if !gen.content.is_empty() {
            match self.store.insert_message(
                gen.session_id,
                Role::Assistant,
                &gen.content,
                reasoning,
            ) {
                Ok(m) => {
                    if self.current_session().map(|s| s.id) == Some(gen.session_id) {
                        self.view.messages.push(m);
                    }
                }
                Err(e) => self.toast(format!("保存回复失败: {e}"), true),
            }
        }
        match error {
            Some(e) => {
                let suffix = if gen.content.is_empty() {
                    ""
                } else {
                    "（已保留部分内容）"
                };
                self.toast(format!("{e} {suffix}"), true);
            }
            None => {
                if let Some(next) = self.queue.pop_front() {
                    self.send_message(&next, tx);
                }
            }
        }
    }

    fn stop_generation(&mut self, tx: &mpsc::Sender<AppEvent>) {
        if let Some(g) = &self.generating {
            g.handle.abort();
        }
        self.finalize_generation(None, tx);
        self.toast("已停止生成", false);
    }

    fn regenerate(&mut self, tx: &mpsc::Sender<AppEvent>) {
        if self.generating.is_some() {
            self.toast("正在生成中", false);
            return;
        }
        let Some(session_id) = self.current_session().map(|s| s.id) else {
            return;
        };
        let Some(i) = self
            .view
            .messages
            .iter()
            .rposition(|m| m.role == Role::User)
        else {
            self.toast("没有可重新生成的消息", false);
            return;
        };
        let msg_id = self.view.messages[i].id;
        // 删除最后一条 user 之后的消息，重发
        if let Err(e) = self.store.delete_messages_after(session_id, msg_id) {
            self.toast(format!("删除历史失败: {e}"), true);
            return;
        }
        match self.store.messages(session_id) {
            Ok(m) => self.view.messages = m,
            Err(e) => {
                self.toast(format!("重载消息失败: {e}"), true);
                return;
            }
        }
        self.chat_scroll = None;
        self.dispatch_generation(tx);
    }

    // ---------- LLM 事件之外的异步结果 ----------

    fn on_models(&mut self, r: Result<Vec<String>, String>) {
        self.models_loading = false;
        let Some(session) = self.current_session() else {
            return;
        };
        let favs = self.config.favorites_of(&session.provider);
        match r {
            Ok(list) if !list.is_empty() || !favs.is_empty() => {
                self.palette = Some(PaletteState::models(list, favs));
                self.palette_kind = PaletteKind::Models;
            }
            Ok(_) => self.toast("提供商未返回任何模型", true),
            Err(e) => {
                // 拉取失败但已有收藏 → 仍可从收藏中选择
                if favs.is_empty() {
                    self.toast(format!("拉取模型失败: {e}"), true);
                } else {
                    self.palette = Some(PaletteState::models(Vec::new(), favs));
                    self.palette_kind = PaletteKind::Models;
                    self.toast(format!("拉取失败: {e}（仅显示收藏）"), true);
                }
            }
        }
    }

    /// 模型面板：收藏/取消选中模型。
    fn toggle_palette_favorite(&mut self) {
        let Some(model) = self
            .palette
            .as_ref()
            .and_then(|p| p.visible.get(p.selected).map(|i| i.label.clone()))
        else {
            return;
        };
        let Some(provider) = self.current_session().map(|s| s.provider.clone()) else {
            return;
        };
        let starred = self.config.toggle_favorite(&provider, &model);
        self.save_config_or_toast();
        // 同步面板条目标记（items 与 visible 都更新）
        if let Some(p) = &mut self.palette {
            for it in p.items.iter_mut().chain(p.visible.iter_mut()) {
                if it.label == model {
                    it.starred = starred;
                }
            }
        }
        self.toast(
            if starred {
                format!("已收藏 {model}")
            } else {
                format!("已取消收藏 {model}")
            },
            false,
        );
    }

    fn on_tick(&mut self, tx: &mpsc::Sender<AppEvent>) {
        if let Some(t) = &self.toast {
            if t.at.elapsed() >= Duration::from_secs(3) {
                self.toast = None;
            }
        }
        // Esc 待定超时：判定为普通 Esc（按下后未跟 '['），结算其功能
        if let Some(at) = self.esc_at {
            if at.elapsed() > Duration::from_millis(120) {
                let probe = self.start_probe.take();
                self.esc_at = None;
                self.do_esc(tx);
                if let Some(chars) = probe {
                    // 回放开始标记中已匹配的字符（[200~ 判定失败）
                    for c in chars {
                        self.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE), tx);
                    }
                }
            }
        }
    }

    /// 执行 Esc 的功能：生成中停止生成，否则焦点回到输入框。
    fn do_esc(&mut self, tx: &mpsc::Sender<AppEvent>) {
        if self.generating.is_some() {
            self.stop_generation(tx);
        } else {
            self.focus = Focus::Input;
        }
    }

    /// 在按键流中重组 bracketed paste 序列。返回 true 表示按键已被粘贴解析消费。
    fn feed_paste(&mut self, key: KeyEvent, tx: &mpsc::Sender<AppEvent>) -> bool {
        // 收集模式：所有按键都喂给解析器，换行不再触发发送
        if let Some(p) = &mut self.paste {
            p.feed(key);
            if p.done {
                let text = self.paste.take().unwrap().finish();
                self.on_paste(&text);
            }
            return true;
        }
        match key.code {
            KeyCode::Esc => {
                // 可能是开始标记 \x1b[200~ 的 ESC，暂缓执行功能
                self.esc_at = Some(Instant::now());
                true
            }
            KeyCode::Char(c) => {
                // 正在匹配开始标记 "[200~"
                if let Some(probe) = &mut self.start_probe {
                    let expect = "200~";
                    let i = probe.len(); // 含开头的 '['
                    if i > 0 && i <= 4 && c == expect.chars().nth(i - 1).unwrap() {
                        probe.push(c);
                        if probe.len() == 5 {
                            self.start_probe = None;
                            self.paste = Some(PasteCollect::new());
                        }
                        return true;
                    }
                    // 判定失败：回退（执行挂起 Esc + 回放已匹配字符）
                    let chars = self.start_probe.take().unwrap();
                    self.esc_at = None;
                    self.do_esc(tx);
                    for ch in chars {
                        self.on_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE), tx);
                    }
                    return false;
                }
                // Esc 待定后收到字符
                if let Some(at) = self.esc_at {
                    if at.elapsed() < Duration::from_millis(300) {
                        if c == '[' {
                            self.esc_at = None;
                            self.start_probe = Some(vec!['[']);
                            return true;
                        }
                        self.esc_at = None;
                        self.do_esc(tx);
                    }
                }
                false
            }
            _ => {
                // Esc 待定后收到非字符键：结算 Esc 功能
                if self.esc_at.is_some() {
                    let probe = self.start_probe.take();
                    self.esc_at = None;
                    self.do_esc(tx);
                    if let Some(chars) = probe {
                        for c in chars {
                            self.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE), tx);
                        }
                    }
                }
                false
            }
        }
    }

    /// 通过系统剪贴板读取文本（Ctrl+V 路径，不依赖终端粘贴事件）。
    fn paste_clipboard(&mut self) {
        match arboard::Clipboard::new() {
            Ok(mut cb) => match cb.get_text() {
                Ok(t) if !t.trim().is_empty() => self.on_paste(&t),
                Ok(_) => self.toast("剪贴板无文本内容", false),
                Err(e) => self.toast(format!("读取剪贴板失败: {e}"), true),
            },
            Err(e) => self.toast(format!("无法访问剪贴板: {e}"), true),
        }
    }

    // ---------- 滚动 ----------

    fn scroll_up(&mut self, n: u16) {
        let max = self.chat_total.saturating_sub(self.chat_height);
        let cur = self.chat_scroll.unwrap_or(max);
        self.chat_scroll = Some(cur.saturating_sub(n));
    }

    fn scroll_down(&mut self, n: u16) {
        let max = self.chat_total.saturating_sub(self.chat_height);
        match self.chat_scroll {
            None => {}
            Some(o) => {
                self.chat_scroll = if o + n >= max { None } else { Some(o + n) };
            }
        }
    }

    // ---------- 命令执行 ----------

    pub fn execute_action(&mut self, action: Action, tx: &mpsc::Sender<AppEvent>) {
        match action {
            Action::NewSession => self.new_session(),
            Action::FocusSidebar => {
                self.focus = Focus::Sidebar;
                self.sidebar_sel = self.view.current.unwrap_or(0);
            }
            Action::SelectModel => self.fetch_models(tx),
            Action::ProviderAdd => {
                self.form = Some(FormState::provider(
                    FormPurpose::ProviderAdd,
                    "",
                    "https://api.example.com/v1",
                    "",
                    "",
                ));
            }
            Action::ProviderEdit => self.open_provider_palette(PaletteKind::ProvidersEdit),
            Action::ProviderDelete => self.open_provider_palette(PaletteKind::ProvidersDelete),
            Action::ProviderSwitch => self.open_provider_palette(PaletteKind::ProvidersSwitch),
            Action::RenameSession => {
                if let Some(s) = self.current_session() {
                    let id = s.id;
                    let title = s.title.clone();
                    self.form = Some(FormState::simple(
                        "重命名会话",
                        FormPurpose::RenameSession { session_id: id },
                        "新标题",
                        &title,
                    ));
                }
            }
            Action::EditSystemPrompt => {
                if let Some(s) = self.current_session() {
                    let id = s.id;
                    let cur = s
                        .system_prompt
                        .clone()
                        .or_else(|| self.config.system_prompt.clone())
                        .unwrap_or_default();
                    self.form = Some(FormState::simple(
                        "系统提示词",
                        FormPurpose::SystemPrompt { session_id: id },
                        "提示词（留空清除）",
                        &cur,
                    ));
                }
            }
            Action::ClearSession => self.clear_session(),
            Action::ExportSession => self.export_session(),
            Action::CopyCode => self.copy_code(),
            Action::Regenerate => self.regenerate(tx),
            Action::ToggleHelp => self.show_help = !self.show_help,
            Action::Quit => self.quitting = true,
            Action::SetModel(m) => self.set_model(m),
            Action::SwitchProvider(n) => self.switch_provider(n),
            Action::RemoveProvider(n) => self.remove_provider(n),
            Action::FavoriteModel => {
                // 收藏/取消当前会话模型
                if let Some(s) = self.current_session() {
                    let (prov, model) = (s.provider.clone(), s.model.clone());
                    if model.is_empty() {
                        self.toast("当前无模型", false);
                    } else {
                        let starred = self.config.toggle_favorite(&prov, &model);
                        self.save_config_or_toast();
                        self.toast(
                            if starred {
                                format!("已收藏 {model}")
                            } else {
                                format!("已取消收藏 {model}")
                            },
                            false,
                        );
                    }
                }
            }
        }
    }

    fn open_provider_palette(&mut self, kind: PaletteKind) {
        let names: Vec<String> = self
            .config
            .providers
            .iter()
            .map(|p| p.name.clone())
            .collect();
        if names.is_empty() {
            self.toast("暂无提供商，请先 /provider add 添加", true);
            return;
        }
        self.palette = Some(PaletteState::providers(Action::SwitchProvider, names));
        self.palette_kind = kind;
    }

    fn open_provider_edit_form(&mut self, name: &str) {
        if let Some(p) = self.config.provider(name) {
            let (name, base_url, model) =
                (p.name.clone(), p.base_url.clone(), p.default_model.clone());
            self.form = Some(FormState::provider(
                FormPurpose::ProviderEdit { name: name.clone() },
                &name,
                &base_url,
                "",
                &model,
            ));
        } else {
            self.toast("提供商不存在", true);
        }
    }

    fn fetch_models(&mut self, tx: &mpsc::Sender<AppEvent>) {
        if self.models_loading {
            self.toast("正在拉取模型列表…", false);
            return;
        }
        let Some(session) = self.current_session().cloned() else {
            return;
        };
        let Some(provider) = self.config.effective_provider(&session.provider) else {
            self.toast(format!("提供商 \"{}\" 不存在", session.provider), true);
            return;
        };
        let is_local =
            provider.base_url.contains("localhost") || provider.base_url.contains("127.0.0.1");
        if provider.api_key.trim().is_empty() && !is_local {
            self.toast(
                format!(
                    "提供商 \"{}\" 的 API Key 为空，无法拉取模型列表",
                    session.provider
                ),
                true,
            );
            return;
        }
        self.models_loading = true;
        self.toast("正在拉取模型列表…", false);
        let client = OpenAiClient::new(&provider, &session.model, self.config.proxy.as_deref());
        let tx = tx.clone();
        tokio::spawn(async move {
            let r = client.list_models().await.map_err(|e| e.to_string());
            let _ = tx.send(AppEvent::Models(r)).await;
        });
    }

    fn set_model(&mut self, model: String) {
        let Some(idx) = self.cur_idx() else { return };
        let id = self.view.sessions[idx].id;
        match self.store.update_session_model(id, &model) {
            Ok(_) => {
                self.view.sessions[idx].model = model.clone();
                // 记住本次使用的模型（新建会话沿用）
                let provider = self.view.sessions[idx].provider.clone();
                self.config.last_provider = Some(provider);
                self.config.last_model = Some(model.clone());
                self.save_config_or_toast();
                self.toast(format!("模型已切换为 {model}"), false);
            }
            Err(e) => self.toast(format!("切换模型失败: {e}"), true),
        }
    }

    fn switch_provider(&mut self, name: String) {
        let Some(p) = self.config.provider(&name).cloned() else {
            self.toast(format!("提供商 {name} 不存在"), true);
            return;
        };
        let Some(idx) = self.cur_idx() else { return };
        let id = self.view.sessions[idx].id;
        if let Err(e) = self.store.update_session_provider(id, &name) {
            self.toast(format!("切换提供商失败: {e}"), true);
            return;
        }
        // 同步切到该提供商的默认模型，并记住本次选择（新建会话沿用）
        let _ = self.store.update_session_model(id, &p.default_model);
        self.view.sessions[idx].provider = name.clone();
        self.view.sessions[idx].model = p.default_model.clone();
        self.config.last_provider = Some(name.clone());
        self.config.last_model = Some(p.default_model.clone());
        self.save_config_or_toast();
        self.toast(format!("已切换到 {name} · {}", p.default_model), false);
    }

    fn remove_provider(&mut self, name: String) {
        let before = self.config.providers.len();
        self.config.providers.retain(|p| p.name != name);
        if self.config.providers.len() == before {
            return;
        }
        if self.config.default_provider == name {
            self.config.default_provider = self
                .config
                .providers
                .first()
                .map(|p| p.name.clone())
                .unwrap_or_default();
        }
        // 清理“上次使用”记录，避免指向已删除的提供商
        if self.config.last_provider.as_deref() == Some(name.as_str()) {
            self.config.last_provider = None;
            self.config.last_model = None;
        }
        self.save_config_or_toast();
        self.toast(format!("已删除提供商 {name}"), false);
        // 当前会话若指向被删提供商 → 自动切到默认
        if let Some(idx) = self.cur_idx() {
            if self.view.sessions[idx].provider == name {
                let def = self.config.default_provider.clone();
                if !def.is_empty() {
                    self.switch_provider(def);
                } else {
                    self.toast("当前无可用提供商，请添加", true);
                }
            }
        }
    }

    // ---------- 会话操作 ----------

    fn new_session(&mut self) {
        let (provider, model) = new_session_provider_model(&self.config);
        match self
            .store
            .create_session(Session::DEFAULT_TITLE, &provider, &model, None)
        {
            Ok(s) => {
                self.view.sessions.insert(0, s);
                self.view.current = Some(0);
                self.view.messages = Vec::new();
                self.sidebar_sel = 0;
                self.chat_scroll = None;
                self.chat_sel = None;
                self.focus = Focus::Input;
            }
            Err(e) => self.toast(format!("新建会话失败: {e}"), true),
        }
    }

    fn select_session(&mut self, i: usize) {
        if self.view.sessions.is_empty() {
            return;
        }
        let i = i.min(self.view.sessions.len() - 1);
        let id = self.view.sessions[i].id;
        self.view.current = Some(i);
        match self.store.messages(id) {
            Ok(m) => self.view.messages = m,
            Err(_) => self.view.messages = Vec::new(),
        }
        self.sidebar_sel = i;
        self.chat_scroll = None;
        self.chat_sel = if self.view.messages.is_empty() {
            None
        } else {
            Some(self.view.messages.len() - 1)
        };
        self.focus = Focus::Input;
    }

    fn delete_selected_session(&mut self) {
        if self.view.sessions.is_empty() {
            return;
        }
        let idx = self.sidebar_sel.min(self.view.sessions.len() - 1);
        let id = self.view.sessions[idx].id;
        let cur_id = self.current_session().map(|s| s.id);
        if let Err(e) = self.store.delete_session(id) {
            self.toast(format!("删除失败: {e}"), true);
            return;
        }
        // 若正在为该会话生成 → 停止
        if self.generating.as_ref().map(|g| g.session_id) == Some(id) {
            if let Some(g) = &self.generating {
                g.handle.abort();
            }
            self.generating = None;
        }
        self.view.sessions = self.store.list_sessions().unwrap_or_default();
        if self.view.sessions.is_empty() {
            self.new_session();
            return;
        }
        self.sidebar_sel = self.sidebar_sel.min(self.view.sessions.len() - 1);
        self.view.current = cur_id
            .and_then(|cid| self.view.sessions.iter().position(|s| s.id == cid))
            .or(Some(self.sidebar_sel));
        self.select_session(self.view.current.unwrap_or(0));
    }

    fn clear_session(&mut self) {
        let Some(session_id) = self.current_session().map(|s| s.id) else {
            return;
        };
        if let Err(e) = self.store.clear_messages(session_id) {
            self.toast(format!("清空失败: {e}"), true);
            return;
        }
        self.view.messages = Vec::new();
        self.chat_scroll = None;
        self.chat_sel = None;
        self.toast("会话消息已清空", false);
    }

    fn export_session(&mut self) {
        let Some(session) = self.current_session().cloned() else {
            return;
        };
        let mut md = format!(
            "# {}\n\n> {} · {}\n\n",
            session.title, session.provider, session.model
        );
        for m in &self.view.messages {
            let h = match m.role {
                Role::User => "## 你",
                Role::Assistant => "## AI",
                Role::System => "## 系统",
            };
            md.push_str(h);
            md.push_str("\n\n");
            if let Some(r) = &m.reasoning {
                if !r.is_empty() {
                    md.push_str(&format!("> 思考：{}\n\n", r.replace('\n', "\n> ")));
                }
            }
            md.push_str(&m.content);
            md.push_str("\n\n");
        }
        let dir = self.data_dir.join("exports");
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.toast(format!("创建导出目录失败: {e}"), true);
            return;
        }
        let path = dir.join(format!(
            "{}-{}.md",
            crate::util::sanitize_filename(&session.title),
            session.id
        ));
        match std::fs::write(&path, md) {
            Ok(_) => self.toast(format!("已导出 {}", path.display()), false),
            Err(e) => self.toast(format!("导出失败: {e}"), true),
        }
    }

    fn copy_code(&mut self) {
        // 优先最近一条 assistant 消息（含生成中的内容）
        let text = self
            .view
            .messages
            .iter()
            .rev()
            .find(|m| m.role == Role::Assistant)
            .map(|m| m.content.clone())
            .or_else(|| self.generating.as_ref().map(|g| g.content.clone()))
            .unwrap_or_default();
        match crate::util::extract_last_code_block(&text) {
            Some(code) => match arboard::Clipboard::new().and_then(|mut c| c.set_text(code)) {
                Ok(_) => self.toast("已复制代码块", false),
                Err(e) => self.toast(format!("复制失败: {e}"), true),
            },
            None => self.toast("没有可复制的代码块", false),
        }
    }

    fn save_config_or_toast(&mut self) {
        if let Err(e) = self.config.save(&self.data_dir) {
            self.toast(format!("保存配置失败: {e}"), true);
        }
    }
}

// ---------- 主循环 ----------

/// 新建会话使用的提供商与模型：
/// 优先沿用上次使用的（且提供商仍存在、模型非空），否则回退默认提供商逻辑。
fn new_session_provider_model(config: &Config) -> (String, String) {
    if let Some(p) = config.last_provider.as_deref() {
        if let Some(prov) = config.provider(p) {
            let model: String = config
                .last_model
                .as_deref()
                .filter(|m| !m.is_empty())
                .unwrap_or(&prov.default_model)
                .to_string();
            return (p.to_string(), model);
        }
    }
    let provider = pick_default_provider(config);
    let model = config
        .provider(&provider)
        .map(|p| p.default_model.clone())
        .unwrap_or_default();
    (provider, model)
}

/// 选择新会话使用的提供商：默认提供商可用（有 key 或本地）则用之，否则回退到任一可用提供商。
fn pick_default_provider(config: &Config) -> String {
    let env_key = std::env::var("TUAI_API_KEY")
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false);
    let usable = |p: &Provider| {
        env_key
            || !p.api_key.trim().is_empty()
            || p.base_url.contains("localhost")
            || p.base_url.contains("127.0.0.1")
    };
    let def = &config.default_provider;
    if config.provider(def).map(usable).unwrap_or(false) {
        return def.clone();
    }
    for p in &config.providers {
        if usable(p) {
            return p.name.clone();
        }
    }
    def.clone()
}

/// 初始化终端并运行事件循环（返回 = 用户退出）。
pub async fn run(config: Config, store: Store, data_dir: PathBuf) -> anyhow::Result<()> {
    let mut terminal = ratatui::init();
    let _ = crossterm::execute!(
        std::io::stdout(),
        crossterm::event::EnableMouseCapture,
        crossterm::event::EnableBracketedPaste
    );
    let mut app = match App::new(config, store, data_dir) {
        Ok(a) => a,
        Err(e) => {
            let _ = crossterm::execute!(
                std::io::stdout(),
                crossterm::event::DisableMouseCapture,
                crossterm::event::DisableBracketedPaste
            );
            ratatui::restore();
            return Err(e);
        }
    };
    let result = drive(&mut terminal, &mut app).await;
    let _ = crossterm::execute!(
        std::io::stdout(),
        crossterm::event::DisableMouseCapture,
        crossterm::event::DisableBracketedPaste
    );
    ratatui::restore();
    result
}

async fn drive(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> anyhow::Result<()> {
    let (tx, mut rx) = mpsc::channel::<AppEvent>(512);
    // 终端事件读取线程：阻塞读 → 转发到主循环
    {
        let tx = tx.clone();
        std::thread::spawn(move || {
            while let Ok(ev) = crossterm::event::read() {
                if tx.blocking_send(AppEvent::Term(ev)).is_err() {
                    break;
                }
            }
        });
    }
    // 250ms tick：toast 过期等
    {
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut iv = tokio::time::interval(Duration::from_millis(250));
            iv.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                iv.tick().await;
                if tx.send(AppEvent::Tick).await.is_err() {
                    break;
                }
            }
        });
    }
    // 事件循环：先渲染，再等事件（无事件时挂起，不空转）
    while !app.quitting {
        terminal.draw(|f| crate::tui::draw(f, app))?;
        match rx.recv().await {
            Some(ev) => app.dispatch(ev, &tx),
            None => break,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn paste_reassembly_multiline() {
        // Windows 拆键流：内容 "a\nb\nc" 的换行变 Enter
        let mut p = PasteCollect::new();
        for code in [
            KeyCode::Char('a'),
            KeyCode::Enter,
            KeyCode::Char('b'),
            KeyCode::Enter,
            KeyCode::Char('c'),
        ] {
            p.feed(key(code));
        }
        assert!(!p.done);
        // 结束标记 \x1b[201~ 的拆键流（先 Esc）
        p.feed(key(KeyCode::Esc));
        for c in ['[', '2', '0', '1', '~'] {
            p.feed(key(KeyCode::Char(c)));
        }
        assert!(p.done);
        assert_eq!(p.finish(), "a\nb\nc");
    }

    #[test]
    fn paste_reassembly_escaped_esc() {
        // 内容含 ESC（转义为 ESC ESC），结束标记不被误判
        let mut p = PasteCollect::new();
        p.feed(key(KeyCode::Esc));
        p.feed(key(KeyCode::Esc));
        p.feed(key(KeyCode::Char('x')));
        assert!(!p.done);
        p.feed(key(KeyCode::Esc));
        for c in ['[', '2', '0', '1', '~'] {
            p.feed(key(KeyCode::Char(c)));
        }
        assert!(p.done);
        assert_eq!(p.finish(), "\x1bx");
    }

    #[test]
    fn paste_reassembly_content_looks_like_end_marker() {
        // 内容里出现 "[201~"（无 ESC 前缀）：不应误判为结束标记
        let mut p = PasteCollect::new();
        for c in ['[', '2', '0', '1', '~'] {
            p.feed(key(KeyCode::Char(c)));
        }
        assert!(!p.done);
        p.feed(key(KeyCode::Esc));
        for c in ['[', '2', '0', '1', '~'] {
            p.feed(key(KeyCode::Char(c)));
        }
        assert!(p.done);
        assert_eq!(p.finish(), "[201~");
    }
}
