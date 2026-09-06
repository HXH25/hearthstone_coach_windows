use super::server::DemoState;

#[cfg(not(windows))]
pub fn run_control_center(_state: DemoState) -> Result<(), String> {
    Err("HearthCoach Control Center native UI currently targets Windows".to_owned())
}

#[cfg(windows)]
mod windows_ui {
    use std::{
        ffi::c_void,
        mem::zeroed,
        ptr::null_mut,
    };

    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, WPARAM},
        Graphics::Gdi::{GetStockObject, UpdateWindow, WHITE_BRUSH},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
            GetWindowTextLengthW, GetWindowTextW, HMENU, KillTimer, MessageBoxW, PostQuitMessage,
            RegisterClassW, SendMessageW, SetTimer, SetWindowTextW, ShowWindow,
            TranslateMessage, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT,
            MB_ICONERROR, MB_ICONINFORMATION, MB_OK, MSG, SW_SHOW, WM_CLOSE, WM_COMMAND,
            WM_DESTROY, WM_TIMER, WNDCLASSW, WS_BORDER, WS_CHILD, WS_OVERLAPPEDWINDOW,
            WS_VISIBLE, WS_VSCROLL,
        },
    };

    use crate::demo::{
        compliance::{now_ms, AgentSessionArchive, SessionSummary, TaskRecord, TaskStatus},
        config::DeepSeekConfig,
    };

    use super::DemoState;

    const ES_AUTOHSCROLL: u32 = 0x0080;
    const ES_AUTOVSCROLL: u32 = 0x0040;
    const ES_MULTILINE: u32 = 0x0004;
    const ES_READONLY: u32 = 0x0800;
    const ES_PASSWORD: u32 = 0x0020;
    const BS_AUTOCHECKBOX: u32 = 0x0000_0003;
    const BM_GETCHECK: u32 = 0x00F0;
    const BM_SETCHECK: u32 = 0x00F1;
    const BST_CHECKED: usize = 1;
    const LB_ADDSTRING: u32 = 0x0180;
    const LB_RESETCONTENT: u32 = 0x0184;
    const LB_GETCURSEL: u32 = 0x0188;
    const LB_SETCURSEL: u32 = 0x0186;
    const LB_ERR: isize = -1;

    const ID_HOME: i32 = 100;
    const ID_MODEL: i32 = 101;
    const ID_HISTORY: i32 = 102;
    const ID_USAGE: i32 = 103;
    const ID_TASKS: i32 = 104;
    const ID_ENVIRONMENT: i32 = 105;
    const ID_SAVE_MODEL: i32 = 200;
    const ID_TEST_MODEL: i32 = 201;
    const ID_REFRESH_HISTORY: i32 = 300;
    const ID_LOAD_HISTORY: i32 = 301;
    const ID_SAVE_SESSION: i32 = 302;
    const ID_RESTORE_HISTORY: i32 = 303;
    const ID_CANCEL_TASK: i32 = 400;
    const ID_CANCEL_ALL: i32 = 401;
    const ID_ENV_REFRESH: i32 = 500;
    const ID_ENV_REPAIR: i32 = 501;
    const ID_ENV_LOG_REFRESH: i32 = 502;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Page {
        Home,
        Model,
        History,
        Usage,
        Tasks,
        Environment,
    }

    #[derive(Default)]
    struct ModelEdits {
        compatibility: HWND,
        endpoint: HWND,
        api_key: HWND,
        model: HWND,
        context_window: HWND,
        max_tokens: HWND,
        timeout: HWND,
        thinking: HWND,
        currency: HWND,
        input_price: HWND,
        output_price: HWND,
        cached_price: HWND,
        token_budget: HWND,
        cost_budget: HWND,
    }

    struct UiContext {
        state: DemoState,
        page: Page,
        children: Vec<HWND>,
        main_text: HWND,
        list: HWND,
        detail: HWND,
        model: ModelEdits,
        history_items: Vec<SessionSummary>,
        task_items: Vec<TaskRecord>,
        last_task_key: String,
        last_environment_refresh_ms: u64,
    }

    impl UiContext {
        fn new(state: DemoState) -> Self {
            Self {
                state,
                page: Page::Home,
                children: Vec::new(),
                main_text: null_mut(),
                list: null_mut(),
                detail: null_mut(),
                model: ModelEdits::default(),
                history_items: Vec::new(),
                task_items: Vec::new(),
                last_task_key: String::new(),
                last_environment_refresh_ms: 0,
            }
        }
    }

    static mut UI_CONTEXT: *mut UiContext = null_mut();

    pub fn run_control_center(state: DemoState) -> Result<(), String> {
        unsafe {
            let class_name = wide("HearthCoachControlCenterClass");
            let title = wide("HearthCoach Control Center · V0.5.0.7");
            let hinstance = GetModuleHandleW(null_mut());
            if hinstance.is_null() {
                return Err("GetModuleHandleW failed".to_owned());
            }
            let wc = WNDCLASSW {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(wnd_proc),
                hInstance: hinstance,
                hbrBackground: GetStockObject(WHITE_BRUSH) as _,
                lpszClassName: class_name.as_ptr(),
                ..zeroed()
            };
            if RegisterClassW(&wc) == 0 {
                // RegisterClassW returns zero if a class with the same name was
                // already registered in-process too; creation below is authoritative.
            }

            let context = Box::new(UiContext::new(state));
            UI_CONTEXT = Box::into_raw(context);
            let hwnd = CreateWindowExW(
                0,
                class_name.as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                1080,
                760,
                null_mut(),
                null_mut(),
                hinstance,
                null_mut(),
            );
            if hwnd.is_null() {
                let _ = Box::from_raw(UI_CONTEXT);
                UI_CONTEXT = null_mut();
                return Err("CreateWindowExW(Control Center) failed".to_owned());
            }
            build_page(hwnd, &mut *UI_CONTEXT, Page::Home);
            SetTimer(hwnd, 1, 500, None);
            ShowWindow(hwnd, SW_SHOW);
            UpdateWindow(hwnd);

            let mut msg: MSG = zeroed();
            while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if !UI_CONTEXT.is_null() {
                let _ = Box::from_raw(UI_CONTEXT);
                UI_CONTEXT = null_mut();
            }
            Ok(())
        }
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_COMMAND => {
                if !UI_CONTEXT.is_null() {
                    let id = (wparam & 0xffff) as i32;
                    handle_command(hwnd, &mut *UI_CONTEXT, id);
                }
                0
            }
            WM_TIMER => {
                if !UI_CONTEXT.is_null() {
                    refresh_page(hwnd, &mut *UI_CONTEXT);
                }
                0
            }
            WM_CLOSE => {
                DestroyWindow(hwnd);
                0
            }
            WM_DESTROY => {
                KillTimer(hwnd, 1);
                PostQuitMessage(0);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    unsafe fn handle_command(hwnd: HWND, ui: &mut UiContext, id: i32) {
        match id {
            ID_HOME => build_page(hwnd, ui, Page::Home),
            ID_MODEL => build_page(hwnd, ui, Page::Model),
            ID_HISTORY => build_page(hwnd, ui, Page::History),
            ID_USAGE => build_page(hwnd, ui, Page::Usage),
            ID_TASKS => build_page(hwnd, ui, Page::Tasks),
            ID_ENVIRONMENT => build_page(hwnd, ui, Page::Environment),
            ID_SAVE_MODEL => match save_model_from_ui(ui) {
                Ok(()) => message(hwnd, "模型配置已保存", false),
                Err(error) => message(hwnd, &error, true),
            },
            ID_TEST_MODEL => match save_model_from_ui(ui) {
                Ok(()) => match ui.state.request_model_test() {
                    Ok(task_id) => message(
                        hwnd,
                        &format!("已保存当前配置并启动连接测试 task #{task_id}，可在任务页查看/取消"),
                        false,
                    ),
                    Err(error) => message(hwnd, &error, true),
                },
                Err(error) => message(hwnd, &error, true),
            },
            ID_REFRESH_HISTORY => build_history_page(hwnd, ui),
            ID_SAVE_SESSION => match ui.state.save_current_session() {
                Ok(path) => {
                    message(hwnd, &format!("会话已保存：{}", path.display()), false);
                    build_history_page(hwnd, ui);
                }
                Err(error) => message(hwnd, &error, true),
            },
            ID_LOAD_HISTORY => load_selected_history(hwnd, ui),
            ID_RESTORE_HISTORY => restore_selected_history(hwnd, ui),
            ID_CANCEL_TASK => cancel_selected_task(hwnd, ui),
            ID_CANCEL_ALL => {
                for task in ui.state.task_records() {
                    if matches!(task.status, TaskStatus::Running | TaskStatus::CancelRequested) {
                        ui.state.cancel_task(task.id);
                    }
                }
                refresh_tasks(ui);
            }
            ID_ENV_REFRESH => {
                if !ui.main_text.is_null() {
                    set_text(ui.main_text, &render_environment(&ui.state));
                }
                ui.last_environment_refresh_ms = now_ms();
            }
            ID_ENV_LOG_REFRESH => {
                let generation = ui.state.request_log_refresh();
                message(
                    hwnd,
                    &format!(
                        "已请求强制刷新 Power.log（generation={generation}）。\n监控线程会立即结束旧日志源并重新打开磁盘上最新的 Power.log。"
                    ),
                    false,
                );
                if !ui.main_text.is_null() {
                    set_text(ui.main_text, &render_environment(&ui.state));
                }
                ui.last_environment_refresh_ms = now_ms();
            }
            ID_ENV_REPAIR => match ui.state.repair_environment() {
                Ok(result) => {
                    let dir = result
                        .status
                        .hearthstone_dir
                        .as_ref()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| "仍未找到".to_owned());
                    let mut text = format!(
                        "环境检测/修复完成。\n炉石目录：{dir}\nPower logging：{}",
                        if result.status.power_logging_ready { "OK" } else { "NOT READY" }
                    );
                    if result.restart_required {
                        text.push_str("\n\n已修改 log.config，并检测到炉石正在运行。请重启炉石一次。 ");
                    }
                    message(hwnd, &text, !result.status.hearthstone_exe_found);
                    if !ui.main_text.is_null() {
                        set_text(ui.main_text, &render_environment(&ui.state));
                    }
                    ui.last_environment_refresh_ms = now_ms();
                }
                Err(error) => message(hwnd, &error, true),
            },
            _ => {}
        }
    }

    unsafe fn build_page(hwnd: HWND, ui: &mut UiContext, page: Page) {
        clear_children(ui);
        ui.page = page;
        build_nav(hwnd, ui);
        match page {
            Page::Home => build_home_page(hwnd, ui),
            Page::Model => build_model_page(hwnd, ui),
            Page::History => build_history_page(hwnd, ui),
            Page::Usage => build_usage_page(hwnd, ui),
            Page::Tasks => build_tasks_page(hwnd, ui),
            Page::Environment => build_environment_page(hwnd, ui),
        }
        refresh_page(hwnd, ui);
    }

    unsafe fn build_nav(hwnd: HWND, ui: &mut UiContext) {
        let labels = [
            ("首页", ID_HOME, Page::Home),
            ("模型配置", ID_MODEL, Page::Model),
            ("历史对局 / AI轨迹", ID_HISTORY, Page::History),
            ("Token / 成本", ID_USAGE, Page::Usage),
            ("实时任务", ID_TASKS, Page::Tasks),
            ("环境诊断", ID_ENVIRONMENT, Page::Environment),
        ];
        let mut x = 18;
        for (label, id, page) in labels {
            let text = if ui.page == page {
                format!("[ {label} ]")
            } else {
                label.to_owned()
            };
            let button = child(
                hwnd,
                "BUTTON",
                &text,
                x,
                14,
                if id == ID_HISTORY { 180 } else { 130 },
                34,
                id,
                0,
            );
            ui.children.push(button);
            x += if id == ID_HISTORY { 190 } else { 140 };
        }
        let heading = child(hwnd, "STATIC", "HearthCoach · HDT式桌面主程序", 18, 58, 480, 28, 0, 0);
        ui.children.push(heading);
    }

    unsafe fn build_home_page(hwnd: HWND, ui: &mut UiContext) {
        ui.main_text = child(
            hwnd,
            "EDIT",
            "",
            18,
            96,
            1020,
            570,
            0,
            ES_MULTILINE | ES_AUTOVSCROLL | ES_READONLY | WS_VSCROLL,
        );
        ui.children.push(ui.main_text);
        let tasks = child(hwnd, "BUTTON", "查看实时任务 / 取消", 18, 680, 220, 34, ID_TASKS, 0);
        let model = child(hwnd, "BUTTON", "配置模型与预算", 250, 680, 200, 34, ID_MODEL, 0);
        let history = child(hwnd, "BUTTON", "查看历史对局", 462, 680, 190, 34, ID_HISTORY, 0);
        let environment = child(hwnd, "BUTTON", "环境诊断", 664, 680, 150, 34, ID_ENVIRONMENT, 0);
        ui.children.extend([tasks, model, history, environment]);
    }

    unsafe fn build_model_page(hwnd: HWND, ui: &mut UiContext) {
        let cfg = ui.state.config().deepseek;
        let fields = [
            ("API模式 (deepseek/openai)", cfg.api_compatibility.clone(), 1000),
            ("API Endpoint", cfg.base_url.clone(), 1001),
            ("API Key（留 ******** 表示不修改）", if cfg.api_key.is_empty() { String::new() } else { "********".to_owned() }, 1002),
            ("Model", cfg.model.clone(), 1003),
            ("Context Window", cfg.context_window.to_string(), 1004),
            ("Max Output Tokens", cfg.max_tokens.to_string(), 1005),
            ("Timeout Seconds", cfg.timeout_seconds.to_string(), 1006),
            ("货币", cfg.pricing.currency.clone(), 1007),
            ("输入价格 / 1M tokens", cfg.pricing.input_per_million.to_string(), 1008),
            ("输出价格 / 1M tokens", cfg.pricing.output_per_million.to_string(), 1009),
            ("缓存输入价格 / 1M", cfg.pricing.cached_input_per_million.to_string(), 1010),
            ("Token预算（0=不限）", cfg.budget.max_total_tokens.unwrap_or(0).to_string(), 1011),
            ("成本预算（0=不限）", cfg.budget.max_cost.unwrap_or(0.0).to_string(), 1012),
        ];
        let mut y = 100;
        let mut handles = Vec::new();
        for (label, value, code) in fields {
            let l = child(hwnd, "STATIC", label, 28, y + 4, 280, 24, 0, 0);
            let mut style = ES_AUTOHSCROLL | WS_BORDER;
            if code == 1002 {
                style |= ES_PASSWORD;
            }
            let e = child(hwnd, "EDIT", &value, 320, y, 600, 28, code, style);
            ui.children.extend([l, e]);
            handles.push((code, e));
            y += 38;
        }
        let thinking = child(
            hwnd,
            "BUTTON",
            "启用 Thinking Mode",
            320,
            y,
            220,
            30,
            1013,
            BS_AUTOCHECKBOX,
        );
        if cfg.thinking {
            SendMessageW(thinking, BM_SETCHECK, BST_CHECKED, 0);
        }
        ui.children.push(thinking);
        y += 44;
        let save = child(hwnd, "BUTTON", "保存配置", 320, y, 150, 34, ID_SAVE_MODEL, 0);
        let test = child(hwnd, "BUTTON", "测试连接", 482, y, 150, 34, ID_TEST_MODEL, 0);
        ui.children.extend([save, test]);
        for (code, handle) in handles {
            match code {
                1000 => ui.model.compatibility = handle,
                1001 => ui.model.endpoint = handle,
                1002 => ui.model.api_key = handle,
                1003 => ui.model.model = handle,
                1004 => ui.model.context_window = handle,
                1005 => ui.model.max_tokens = handle,
                1006 => ui.model.timeout = handle,
                1007 => ui.model.currency = handle,
                1008 => ui.model.input_price = handle,
                1009 => ui.model.output_price = handle,
                1010 => ui.model.cached_price = handle,
                1011 => ui.model.token_budget = handle,
                1012 => ui.model.cost_budget = handle,
                _ => {}
            }
        }
        ui.model.thinking = thinking;
    }

    unsafe fn build_history_page(hwnd: HWND, ui: &mut UiContext) {
        clear_page_body(ui);
        // build_nav was removed by clear_page_body, so recreate the entire page.
        ui.page = Page::History;
        build_nav(hwnd, ui);
        ui.list = child(hwnd, "LISTBOX", "", 18, 100, 360, 530, 310, WS_VSCROLL | WS_BORDER);
        ui.detail = child(
            hwnd,
            "EDIT",
            "选择一局历史对局后点击“加载查看”。\r\n历史 JSON 会保存 GameArchive + RoundPlan + Chat + API calls + Tasks + Trace。",
            392,
            100,
            646,
            530,
            0,
            ES_MULTILINE | ES_AUTOVSCROLL | ES_READONLY | WS_VSCROLL,
        );
        ui.children.extend([ui.list, ui.detail]);
        let refresh = child(hwnd, "BUTTON", "刷新列表", 18, 646, 130, 34, ID_REFRESH_HISTORY, 0);
        let load = child(hwnd, "BUTTON", "加载查看", 160, 646, 130, 34, ID_LOAD_HISTORY, 0);
        let save = child(hwnd, "BUTTON", "保存当前会话", 302, 646, 150, 34, ID_SAVE_SESSION, 0);
        let restore = child(hwnd, "BUTTON", "恢复Agent上下文", 464, 646, 170, 34, ID_RESTORE_HISTORY, 0);
        ui.children.extend([refresh, load, save, restore]);
        populate_history(ui);
    }

    unsafe fn build_usage_page(hwnd: HWND, ui: &mut UiContext) {
        ui.main_text = child(
            hwnd,
            "EDIT",
            "",
            18,
            100,
            1020,
            585,
            0,
            ES_MULTILINE | ES_AUTOVSCROLL | ES_READONLY | WS_VSCROLL,
        );
        ui.children.push(ui.main_text);
    }

    unsafe fn build_tasks_page(hwnd: HWND, ui: &mut UiContext) {
        ui.list = child(hwnd, "LISTBOX", "", 18, 100, 600, 520, 410, WS_VSCROLL | WS_BORDER);
        ui.detail = child(
            hwnd,
            "EDIT",
            "选择一个任务；长任务会实时更新阶段/进度，可立即取消。",
            632,
            100,
            406,
            520,
            0,
            ES_MULTILINE | ES_AUTOVSCROLL | ES_READONLY | WS_VSCROLL,
        );
        ui.children.extend([ui.list, ui.detail]);
        let cancel = child(hwnd, "BUTTON", "取消选中任务", 18, 640, 170, 34, ID_CANCEL_TASK, 0);
        let cancel_all = child(hwnd, "BUTTON", "取消全部运行任务", 200, 640, 180, 34, ID_CANCEL_ALL, 0);
        ui.children.extend([cancel, cancel_all]);
        refresh_tasks(ui);
    }

    unsafe fn build_environment_page(hwnd: HWND, ui: &mut UiContext) {
        ui.main_text = child(
            hwnd,
            "EDIT",
            "",
            18,
            100,
            1020,
            540,
            0,
            ES_MULTILINE | ES_AUTOVSCROLL | ES_READONLY | WS_VSCROLL,
        );
        ui.children.push(ui.main_text);
        let refresh = child(hwnd, "BUTTON", "重新检测", 18, 656, 150, 34, ID_ENV_REFRESH, 0);
        let repair = child(
            hwnd,
            "BUTTON",
            "自动检测并修复",
            180,
            656,
            190,
            34,
            ID_ENV_REPAIR,
            0,
        );
        let refresh_log = child(
            hwnd,
            "BUTTON",
            "刷新日志",
            382,
            656,
            150,
            34,
            ID_ENV_LOG_REFRESH,
            0,
        );
        ui.children.extend([refresh, repair, refresh_log]);
    }

    unsafe fn refresh_page(_hwnd: HWND, ui: &mut UiContext) {
        match ui.page {
            Page::Home => {
                if !ui.main_text.is_null() {
                    set_text(ui.main_text, &render_home(&ui.state));
                }
            }
            Page::Usage => {
                if !ui.main_text.is_null() {
                    set_text(ui.main_text, &render_usage(&ui.state));
                }
            }
            Page::Tasks => refresh_tasks(ui),
            Page::History => {}
            Page::Model => {}
            Page::Environment => {
                let now = now_ms();
                if now.saturating_sub(ui.last_environment_refresh_ms) >= 2_000 {
                    if !ui.main_text.is_null() {
                        set_text(ui.main_text, &render_environment(&ui.state));
                    }
                    ui.last_environment_refresh_ms = now;
                }
            }
        }
    }

    fn render_home(state: &DemoState) -> String {
        let public = state.public_state();
        let usage = state.usage_summary();
        let running = state
            .task_records()
            .into_iter()
            .filter(|task| matches!(task.status, TaskStatus::Running | TaskStatus::CancelRequested))
            .collect::<Vec<_>>();
        let mut out = String::new();
        out.push_str("HearthCoach 当前状态\r\n\r\n");
        out.push_str(&format!("炉石对局：{}\r\n", if public.match_active { "已连接 / 进行中" } else { "等待对局" }));
        out.push_str(&format!("当前回合：{}   Phase：{}\r\n", public.current_round, public.current_phase));
        out.push_str(&format!("Agent：{}\r\n", public.ai_status));
        out.push_str(&format!("模型：{}\r\nEndpoint：{}\r\n", public.model, public.base_url));
        out.push_str(&format!("Session：{}\r\n\r\n", public.session_id.unwrap_or_else(|| "-".to_owned())));
        out.push_str("本会话用量\r\n");
        out.push_str(&format!("Input tokens : {}\r\n", usage.usage.prompt_tokens));
        out.push_str(&format!("Output tokens: {}\r\n", usage.usage.completion_tokens));
        out.push_str(&format!("Total tokens : {}\r\n", usage.usage.total_tokens));
        out.push_str(&format!("Cost         : {:.6} {}\r\n", usage.cost.total_cost, usage.cost.currency));
        if let Some(ratio) = usage.token_budget_ratio {
            out.push_str(&format!("Token budget : {:.1}%\r\n", ratio * 100.0));
        }
        if let Some(ratio) = usage.cost_budget_ratio {
            out.push_str(&format!("Cost budget  : {:.1}%\r\n", ratio * 100.0));
        }
        out.push_str("\r\n实时任务\r\n");
        if running.is_empty() {
            out.push_str("暂无长任务。\r\n");
        } else {
            for task in running {
                let elapsed_s = now_ms().saturating_sub(task.started_at_ms) as f64 / 1000.0;
                out.push_str(&format!(
                    "#{} {} · {}% · {} · 已运行 {:.1}s · {}\r\n",
                    task.id, task.label, task.progress_percent, task.stage, elapsed_s, task.detail
                ));
            }
        }
        out.push_str("\r\n最近 Agent 轨迹\r\n");
        for event in state.trace_events().into_iter().rev().take(12).rev() {
            out.push_str(&format!(
                "R{:?} [{}] {} — {}\r\n",
                event.round_number,
                event.category,
                event.title,
                compact_preview(&event.detail, 180),
            ));
        }
        out
    }

    fn render_environment(state: &DemoState) -> String {
        let status = state.environment_status();
        let mut out = String::new();
        out.push_str("HearthCoach 环境诊断\r\n\r\n");
        let hs_dir = status
            .hearthstone_dir
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "未找到".to_owned());
        out.push_str(&format!(
            "Hearthstone   : {}  {}\r\n",
            if status.hearthstone_exe_found { "OK" } else { "MISSING" },
            hs_dir
        ));
        out.push_str(&format!(
            "检测来源      : {}\r\n",
            status.detected_by.as_deref().unwrap_or("-")
        ));
        out.push_str(&format!("HDT / Card DB  : {}\r\n", state.card_catalog_source()));
        out.push_str(&format!(
            "log.config    : {}  {}\r\n",
            if status.log_config_exists { "FOUND" } else { "MISSING" },
            status
                .log_config_path
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "-".to_owned())
        ));
        out.push_str(&format!(
            "Power logging : {}\r\n",
            if status.power_logging_ready { "READY" } else { "NEEDS REPAIR" }
        ));
        out.push_str(&format!(
            "Logs directory: {}\r\n",
            status
                .logs_dir
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "-".to_owned())
        ));
        if let Some(path) = status.latest_power_log.as_ref() {
            let age = status
                .latest_power_log_age_ms
                .map(|ms| format!("{:.1}s ago", ms as f64 / 1000.0))
                .unwrap_or_else(|| "unknown age".to_owned());
            out.push_str(&format!("Power.log      : {}\r\n", path.display()));
            out.push_str(&format!(
                "最后更新       : {}  ({})\r\n",
                age,
                if status.power_log_recent { "正在实时写入" } else { "当前未实时写入；未进游戏时正常" }
            ));
        } else {
            out.push_str("Power.log      : 尚未生成\r\n");
        }

        let watched = state.watched_power_log();
        out.push_str(&format!(
            "当前监听日志 : {}\r\n",
            watched
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "-".to_owned())
        ));
        if watched.as_ref() != status.latest_power_log.as_ref() {
            out.push_str("⚠ 当前监听日志不是磁盘上的最新 Power.log；监控线程会自动切换。也可点击“刷新日志”立即强制重扫。\r\n");
        }

        out.push_str("\r\n状态说明\r\n");
        if status.hearthstone_exe_found && status.power_logging_ready {
            out.push_str("基础环境已就绪。进入炉石后 Power.log 应开始更新，Harness 会自动监听。\r\n");
        } else {
            out.push_str("环境尚未就绪。点击“自动检测并修复”。\r\n");
        }
        for note in status.notes {
            out.push_str(&format!("- {note}\r\n"));
        }
        out.push_str("\r\n如果刚刚修复了 log.config 且炉石已经打开，需要重启炉石一次。\r\n");
        out.push_str("无需再手工把路径改成 D:\\Hearthstone；程序会优先使用真实 Hearthstone.exe 所在目录。\r\n");
        out.push_str("如果怀疑监控卡在旧日志，点击“刷新日志”会强制重置 live watcher 并重新打开最新 Power.log。\r\n");
        out
    }

    fn compact_preview(text: &str, max_chars: usize) -> String {
        let normalized = text
            .replace(['\r', '\n', '\t'], " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if normalized.chars().count() <= max_chars {
            return normalized;
        }
        let mut preview = normalized.chars().take(max_chars.saturating_sub(1)).collect::<String>();
        preview.push('…');
        preview
    }

    fn render_usage(state: &DemoState) -> String {
        let summary = state.usage_summary();
        let calls = state.api_call_records();
        let mut out = String::new();
        out.push_str("Token / 成本统计（来自 API response.usage）\r\n\r\n");
        out.push_str(&format!("API calls : {}\r\n", summary.api_call_count));
        out.push_str(&format!("Input     : {}\r\n", summary.usage.prompt_tokens));
        out.push_str(&format!("  cache hit : {}\r\n", summary.usage.prompt_cache_hit_tokens));
        out.push_str(&format!("Output    : {}\r\n", summary.usage.completion_tokens));
        out.push_str(&format!("Total     : {}\r\n", summary.usage.total_tokens));
        out.push_str(&format!("Cost      : {:.6} {}\r\n", summary.cost.total_cost, summary.cost.currency));
        out.push_str(&format!("  uncached input : {:.6} {}\r\n", summary.cost.uncached_input_cost, summary.cost.currency));
        out.push_str(&format!("  cached input   : {:.6} {}\r\n", summary.cost.cached_input_cost, summary.cost.currency));
        out.push_str(&format!("  output         : {:.6} {}\r\n", summary.cost.output_cost, summary.cost.currency));
        out.push_str(&format!("Token budget: {:?}  ratio={:?}\r\n", summary.token_budget, summary.token_budget_ratio.map(|r| r * 100.0)));
        out.push_str(&format!("Cost budget : {:?}  ratio={:?}\r\n", summary.cost_budget, summary.cost_budget_ratio.map(|r| r * 100.0)));
        out.push_str(&format!("Budget exhausted: {}\r\n\r\n", summary.budget_exhausted));
        out.push_str("每次 API 调用（精确 usage 来自 provider response）\r\n");
        for call in calls.iter().rev().take(100) {
            out.push_str(&format!(
                "#{} task={:?} {} | in={} out={} total={} | {:.6} {} | {}\r\n",
                call.id,
                call.task_id,
                call.kind,
                call.usage.prompt_tokens,
                call.usage.completion_tokens,
                call.usage.total_tokens,
                call.cost.total_cost,
                call.cost.currency,
                call.status,
            ));
        }
        out
    }

    unsafe fn populate_history(ui: &mut UiContext) {
        SendMessageW(ui.list, LB_RESETCONTENT, 0, 0);
        ui.history_items = ui.state.session_summaries().unwrap_or_default();
        for session in &ui.history_items {
            let text = format!(
                "{} | place={:?} | rounds={} | {} | {} tokens | {:.4} {}",
                session.session_id,
                session.final_place,
                session.rounds,
                session.selected_composition.as_deref().unwrap_or("未选择阵容"),
                session.total_tokens,
                session.total_cost,
                session.currency,
            );
            let w = wide(&text);
            SendMessageW(ui.list, LB_ADDSTRING, 0, w.as_ptr() as isize);
        }
    }

    unsafe fn load_selected_history(hwnd: HWND, ui: &mut UiContext) {
        let index = SendMessageW(ui.list, LB_GETCURSEL, 0, 0);
        if index == LB_ERR || index < 0 || index as usize >= ui.history_items.len() {
            message(hwnd, "请先选择一局历史对局", true);
            return;
        }
        let summary = ui.history_items[index as usize].clone();
        match ui.state.load_session_for_review(&summary.path) {
            Ok(session) => set_text(ui.detail, &render_session(&session)),
            Err(error) => message(hwnd, &error, true),
        }
    }

    unsafe fn restore_selected_history(hwnd: HWND, ui: &mut UiContext) {
        let index = SendMessageW(ui.list, LB_GETCURSEL, 0, 0);
        if index == LB_ERR || index < 0 || index as usize >= ui.history_items.len() {
            message(hwnd, "请先选择一局历史对局", true);
            return;
        }
        let summary = ui.history_items[index as usize].clone();
        match ui.state.restore_session_context(&summary.path) {
            Ok(()) => {
                message(hwnd, "历史 Agent 上下文已恢复到审计模式（不会伪造实时 Harness）", false);
                if let Some(session) = ui.state.loaded_session() {
                    set_text(ui.detail, &render_session(&session));
                }
            }
            Err(error) => message(hwnd, &error, true),
        }
    }

    fn render_session(session: &AgentSessionArchive) -> String {
        let mut out = String::new();
        out.push_str(&format!("Session: {}\r\n", session.session_id));
        out.push_str(&format!("Model: {} @ {}\r\n", session.model.model, session.model.base_url));
        out.push_str(&format!("Tokens: {}  Cost: {:.6} {}\r\n", session.usage.usage.total_tokens, session.usage.cost.total_cost, session.usage.cost.currency));
        if let Some(game) = session.game_archive.as_ref() {
            out.push_str(&format!("Final place: {:?}  Rounds: {}\r\n", game.final_place, game.rounds.len()));
        }
        out.push_str(&format!("Composition: {}\r\n", session.selected_composition.as_ref().map(|item| item.name.as_str()).unwrap_or("-")));
        if let Some(plan) = session.round_plan.as_ref() {
            out.push_str("\r\n=== Latest RoundPlan ===\r\n");
            out.push_str(&serde_json::to_string_pretty(plan).unwrap_or_else(|_| "<serialize failed>".to_owned()));
            out.push_str("\r\n");
        }
        if let Some(plan) = session.trinket_plan.as_ref() {
            out.push_str("\r\n=== Latest TrinketPlan ===\r\n");
            out.push_str(&serde_json::to_string_pretty(plan).unwrap_or_else(|_| "<serialize failed>".to_owned()));
            out.push_str("\r\n");
        }
        if let Some(plan) = session.tactical_plan.as_ref() {
            out.push_str("\r\n=== Latest TacticalPlan ===\r\n");
            out.push_str(&serde_json::to_string_pretty(plan).unwrap_or_else(|_| "<serialize failed>".to_owned()));
            out.push_str("\r\n");
        }
        if !session.replan_events.is_empty() {
            out.push_str("\r\n=== Replan Events ===\r\n");
            for event in &session.replan_events {
                out.push_str(&format!("{:?} {} — {}\r\n", event.level, event.code, event.detail));
            }
        }
        out.push_str("\r\n=== Task History ===\r\n");
        for task in &session.tasks {
            out.push_str(&format!(
                "#{} {:?} {} · {}% · {}\r\n  {}\r\n",
                task.id, task.status, task.label, task.progress_percent, task.stage, task.detail
            ));
        }
        out.push_str("\r\n=== Agent Trace（可审计思路/工作流，不是私有CoT）===\r\n");
        for event in &session.trace {
            let elapsed_s = event.timestamp_ms.saturating_sub(session.started_at_ms) as f64 / 1000.0;
            out.push_str(&format!(
                "+{:.1}s R{:?} [{}] {}\r\n  {}\r\n",
                elapsed_s, event.round_number, event.category, event.title, event.detail
            ));
        }
        out.push_str("\r\n=== Chat ===\r\n");
        for message in &session.chat_messages {
            out.push_str(&format!("{}: {}\r\n", message.role, message.content));
        }
        out.push_str("\r\n=== API Calls ===\r\n");
        for call in &session.api_calls {
            out.push_str(&format!(
                "#{} {} in={} out={} cost={:.6} {} status={}\r\n",
                call.id, call.kind, call.usage.prompt_tokens, call.usage.completion_tokens,
                call.cost.total_cost, call.cost.currency, call.status
            ));
        }
        out
    }

    unsafe fn refresh_tasks(ui: &mut UiContext) {
        if ui.list.is_null() {
            return;
        }
        let previous_index = SendMessageW(ui.list, LB_GETCURSEL, 0, 0);
        let previous_id = if previous_index != LB_ERR
            && previous_index >= 0
            && (previous_index as usize) < ui.task_items.len()
        {
            Some(ui.task_items[previous_index as usize].id)
        } else {
            None
        };
        let tasks = ui.state.task_records();
        let key = tasks
            .iter()
            .map(|task| format!("{}:{:?}:{}:{}", task.id, task.status, task.progress_percent, task.stage))
            .collect::<Vec<_>>()
            .join("|");
        if key != ui.last_task_key {
            SendMessageW(ui.list, LB_RESETCONTENT, 0, 0);
            ui.task_items = tasks;
            for task in &ui.task_items {
                let elapsed_s = task
                    .finished_at_ms
                    .unwrap_or_else(now_ms)
                    .saturating_sub(task.started_at_ms) as f64
                    / 1000.0;
                let text = format!(
                    "#{} {:?} {} · {}% · {} · {:.1}s",
                    task.id, task.status, task.label, task.progress_percent, task.stage, elapsed_s
                );
                let w = wide(&text);
                SendMessageW(ui.list, LB_ADDSTRING, 0, w.as_ptr() as isize);
            }
            if let Some(id) = previous_id {
                if let Some(index) = ui.task_items.iter().position(|task| task.id == id) {
                    SendMessageW(ui.list, LB_SETCURSEL, index, 0);
                }
            }
            ui.last_task_key = key;
        }
        if !ui.detail.is_null() {
            let selected_index = SendMessageW(ui.list, LB_GETCURSEL, 0, 0);
            let selected = if selected_index != LB_ERR
                && selected_index >= 0
                && (selected_index as usize) < ui.task_items.len()
            {
                ui.task_items.get(selected_index as usize)
            } else {
                ui.task_items
                    .iter()
                    .rev()
                    .find(|task| matches!(task.status, TaskStatus::Running | TaskStatus::CancelRequested))
            };
            if let Some(task) = selected {
                set_text(
                    ui.detail,
                    &format!(
                        "Task #{}\r\n类型：{}\r\n阶段：{}\r\n进度：{}%\r\n状态：{:?}\r\n已运行：{:.1}s\r\n详情：{}\r\n\r\n用户可随时点击取消；HTTP请求使用可中断的 async reqwest future。",
                        task.id,
                        task.kind,
                        task.stage,
                        task.progress_percent,
                        task.status,
                        task.finished_at_ms.unwrap_or_else(now_ms).saturating_sub(task.started_at_ms) as f64 / 1000.0,
                        task.detail
                    ),
                );
            }
        }
    }

    unsafe fn cancel_selected_task(hwnd: HWND, ui: &mut UiContext) {
        let index = SendMessageW(ui.list, LB_GETCURSEL, 0, 0);
        if index == LB_ERR || index < 0 || index as usize >= ui.task_items.len() {
            message(hwnd, "请先选择任务", true);
            return;
        }
        let task = &ui.task_items[index as usize];
        if ui.state.cancel_task(task.id) {
            message(hwnd, &format!("已请求取消 task #{}", task.id), false);
        } else {
            message(hwnd, "该任务已结束或不可取消", true);
        }
        refresh_tasks(ui);
    }

    unsafe fn save_model_from_ui(ui: &mut UiContext) -> Result<(), String> {
        let old = ui.state.config().deepseek;
        let mut cfg: DeepSeekConfig = old.clone();
        cfg.api_compatibility = get_text(ui.model.compatibility).trim().to_owned();
        cfg.base_url = get_text(ui.model.endpoint).trim().to_owned();
        let key = get_text(ui.model.api_key);
        cfg.api_key = if key.trim() == "********" {
            old.api_key.clone()
        } else {
            key.trim().to_owned()
        };
        cfg.model = get_text(ui.model.model).trim().to_owned();
        cfg.context_window = parse_u32(&get_text(ui.model.context_window), "Context Window")?;
        cfg.max_tokens = parse_u32(&get_text(ui.model.max_tokens), "Max Output Tokens")?;
        cfg.timeout_seconds = parse_u64(&get_text(ui.model.timeout), "Timeout")?;
        cfg.thinking = SendMessageW(ui.model.thinking, BM_GETCHECK, 0, 0) as usize == BST_CHECKED;
        cfg.pricing.currency = get_text(ui.model.currency).trim().to_owned();
        cfg.pricing.input_per_million = parse_f64(&get_text(ui.model.input_price), "输入价格")?;
        cfg.pricing.output_per_million = parse_f64(&get_text(ui.model.output_price), "输出价格")?;
        cfg.pricing.cached_input_per_million = parse_f64(&get_text(ui.model.cached_price), "缓存价格")?;
        let token_budget = parse_u64(&get_text(ui.model.token_budget), "Token预算")?;
        cfg.budget.max_total_tokens = (token_budget > 0).then_some(token_budget);
        let cost_budget = parse_f64(&get_text(ui.model.cost_budget), "成本预算")?;
        cfg.budget.max_cost = (cost_budget > 0.0).then_some(cost_budget);
        if cfg.api_compatibility.is_empty() || cfg.base_url.is_empty() || cfg.model.is_empty() {
            return Err("API模式、Endpoint、Model 不能为空".to_owned());
        }
        ui.state.update_model_config(cfg)
    }

    fn parse_u32(value: &str, name: &str) -> Result<u32, String> {
        value.trim().parse::<u32>().map_err(|_| format!("{name} 不是有效整数"))
    }
    fn parse_u64(value: &str, name: &str) -> Result<u64, String> {
        value.trim().parse::<u64>().map_err(|_| format!("{name} 不是有效整数"))
    }
    fn parse_f64(value: &str, name: &str) -> Result<f64, String> {
        value.trim().parse::<f64>().map_err(|_| format!("{name} 不是有效数字"))
    }

    unsafe fn clear_children(ui: &mut UiContext) {
        for hwnd in ui.children.drain(..) {
            if !hwnd.is_null() {
                DestroyWindow(hwnd);
            }
        }
        ui.main_text = null_mut();
        ui.list = null_mut();
        ui.detail = null_mut();
        ui.model = ModelEdits::default();
        ui.last_task_key.clear();
    }

    unsafe fn clear_page_body(ui: &mut UiContext) {
        clear_children(ui);
    }

    unsafe fn child(
        parent: HWND,
        class_name: &str,
        text: &str,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        id: i32,
        extra_style: u32,
    ) -> HWND {
        let class = wide(class_name);
        let value = wide(text);
        CreateWindowExW(
            0,
            class.as_ptr(),
            value.as_ptr(),
            WS_CHILD | WS_VISIBLE | extra_style,
            x,
            y,
            width,
            height,
            parent,
            menu_id(id),
            null_mut(),
            null_mut(),
        )
    }

    fn menu_id(id: i32) -> HMENU {
        id as usize as *mut c_void
    }

    unsafe fn get_text(hwnd: HWND) -> String {
        if hwnd.is_null() {
            return String::new();
        }
        let len = GetWindowTextLengthW(hwnd).max(0) as usize;
        let mut buf = vec![0u16; len + 1];
        let written = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32).max(0) as usize;
        String::from_utf16_lossy(&buf[..written])
    }

    unsafe fn set_text(hwnd: HWND, text: &str) {
        if hwnd.is_null() {
            return;
        }
        let value = wide(text);
        SetWindowTextW(hwnd, value.as_ptr());
    }

    unsafe fn message(hwnd: HWND, text: &str, error: bool) {
        let title = wide(if error { "HearthCoach · 错误" } else { "HearthCoach" });
        let body = wide(text);
        MessageBoxW(
            hwnd,
            body.as_ptr(),
            title.as_ptr(),
            MB_OK | if error { MB_ICONERROR } else { MB_ICONINFORMATION },
        );
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

#[cfg(windows)]
pub use windows_ui::run_control_center;
