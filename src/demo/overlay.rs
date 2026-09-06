use super::{config::OverlayConfig, server::DemoState};

#[cfg(windows)]
pub fn spawn_overlay(state: DemoState) {
    std::thread::spawn(move || unsafe {
        if let Err(error) = run_windows_overlay(state) {
            eprintln!("[overlay] {error}");
        }
    });
}

#[cfg(not(windows))]
pub fn spawn_overlay(_state: DemoState) {
    eprintln!("[overlay] native in-game overlay is only enabled on Windows");
}

#[cfg(windows)]
#[derive(Debug, Clone)]
enum PanelAction {
    Analyze,
    SetPanelPage(PanelPage),
    SelectComposition(String),
    ResetComposition,
    SetStage(String),
    ToggleCard(String),
    ScrollUp,
    ScrollDown,
    ToggleTestFrames,
    ResetPanelLayout,
    OpenChat,
}


#[cfg(windows)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PanelPage {
    Decision,
    Guide,
}

#[cfg(windows)]
#[derive(Debug, Clone, Copy)]
struct UiRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[cfg(windows)]
impl UiRect {
    fn contains(self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
}

#[cfg(windows)]
#[derive(Clone, Copy)]
enum PanelPointerMode {
    Drag {
        grab_x: i32,
        grab_y: i32,
    },
    Resize {
        start_cursor_x: i32,
        start_cursor_y: i32,
        start_left: i32,
        start_top: i32,
        start_width: i32,
        start_height: i32,
    },
}

#[cfg(windows)]
#[derive(Debug, Clone)]
struct HitRegion {
    rect: UiRect,
    action: PanelAction,
}

#[cfg(windows)]
struct PanelUiState {
    page: PanelPage,
    stage_override: Option<String>,
    expanded_card_id: Option<String>,
    scroll: usize,
    selected_composition_id: Option<String>,
}

#[cfg(windows)]
impl Default for PanelUiState {
    fn default() -> Self {
        Self {
            page: PanelPage::Guide,
            stage_override: None,
            expanded_card_id: None,
            scroll: 0,
            selected_composition_id: None,
        }
    }
}

#[cfg(windows)]
impl PanelUiState {
    fn sync_selection(&mut self, selected: Option<&str>) {
        if self.selected_composition_id.as_deref() != selected {
            self.selected_composition_id = selected.map(ToOwned::to_owned);
            self.page = if selected.is_some() {
                PanelPage::Decision
            } else {
                PanelPage::Guide
            };
            self.stage_override = None;
            self.expanded_card_id = None;
            self.scroll = 0;
        }
    }
}

#[cfg(windows)]
unsafe fn run_windows_overlay(state: DemoState) -> Result<(), String> {
    use std::{
        ptr::{null, null_mut},
        time::Duration,
    };
    use windows_sys::Win32::{
        Foundation::{HWND, POINT, RECT},
        UI::{
            Input::KeyboardAndMouse::{GetAsyncKeyState, SetFocus},
            WindowsAndMessaging::{
                CreateWindowExW, GetCursorPos, GetForegroundWindow, GetWindowTextLengthW,
                GetWindowTextW, SendMessageW, SetForegroundWindow, SetLayeredWindowAttributes,
                SetWindowTextW, SetWindowPos, ShowWindow, HWND_TOPMOST, LWA_ALPHA, LWA_COLORKEY, SW_HIDE,
                SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_SHOWWINDOW, WS_EX_LAYERED,
                WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
            },
        },
    };

    const VK_LBUTTON: i32 = 0x01;
    const SS_NOTIFY: u32 = 0x0100;
    const SW_SHOW: i32 = 5;
    // Built-in EDIT control styles; numeric values keep the dependency surface
    // small while still allowing proper Windows/IME text input.
    const WS_CHILD_STYLE: u32 = 0x4000_0000;
    const WS_CLIPCHILDREN_STYLE: u32 = 0x0200_0000;
    const WS_VISIBLE_STYLE: u32 = 0x1000_0000;
    const WS_BORDER_STYLE: u32 = 0x0080_0000;
    const ES_MULTILINE: u32 = 0x0004;
    const ES_AUTOVSCROLL: u32 = 0x0040;
    const ES_READONLY: u32 = 0x0800;
    const ES_WANTRETURN: u32 = 0x1000;
    const WS_VSCROLL_STYLE: u32 = 0x0020_0000;
    const EM_SETSEL: u32 = 0x00B1;
    const EM_SCROLLCARET: u32 = 0x00B7;
    let class_name = wide("STATIC");

    let highlight_title = wide("HearthCoachHighlightOverlay");
    let highlight: HWND = CreateWindowExW(
        WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
        class_name.as_ptr(), highlight_title.as_ptr(), WS_POPUP,
        0, 0, 10, 10, null_mut(), null_mut(), null_mut(), null(),
    );
    if highlight.is_null() {
        return Err("CreateWindowExW(highlight) failed".to_owned());
    }
    if SetLayeredWindowAttributes(highlight, 0, 255, LWA_COLORKEY) == 0 {
        return Err("SetLayeredWindowAttributes(highlight) failed".to_owned());
    }

    let panel_title = wide("HearthCoachDecisionOverlay");
    let panel: HWND = CreateWindowExW(
        WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
        class_name.as_ptr(), panel_title.as_ptr(), WS_POPUP | SS_NOTIFY,
        0, 0, 320, 260, null_mut(), null_mut(), null_mut(), null(),
    );
    if panel.is_null() {
        return Err("CreateWindowExW(panel) failed".to_owned());
    }

    // A separate horizontal guide window keeps the persistent watchlist away
    // from the compact decision card. It is interactive, so stage/composition
    // controls remain usable without growing the bottom-right panel again.
    let guide_title = wide("HearthCoachGuideOverlay");
    let guide: HWND = CreateWindowExW(
        WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
        class_name.as_ptr(), guide_title.as_ptr(), WS_POPUP | SS_NOTIFY,
        0, 0, 900, 130, null_mut(), null_mut(), null_mut(), null(),
    );
    if guide.is_null() {
        return Err("CreateWindowExW(guide) failed".to_owned());
    }

    // Chat intentionally uses a focusable top-level popup. The normal overlay
    // stays no-activate; only when the player explicitly opens AI chat do we
    // temporarily take keyboard focus so Chinese IME/input works correctly.
    let chat_title = wide("HearthCoachChatOverlay");
    let chat: HWND = CreateWindowExW(
        // Deliberately NOT layered: native EDIT controls on a repeatedly painted
        // layered parent are fragile and caused typed text to disappear in V0.4.5.
        // A normal topmost tool window + WS_CLIPCHILDREN gives the editor its own
        // reliable paint/input surface while the gameplay overlays remain layered.
        WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
        class_name.as_ptr(), chat_title.as_ptr(), WS_POPUP | SS_NOTIFY | WS_CLIPCHILDREN_STYLE,
        0, 0, 480, 390, null_mut(), null_mut(), null_mut(), null(),
    );
    if chat.is_null() {
        return Err("CreateWindowExW(chat) failed".to_owned());
    }
    let edit_class = wide("EDIT");
    let empty_text = wide("");
    // Full chat history is a native read-only EDIT control. Windows handles
    // word wrapping + vertical scrolling, so assistant replies are no longer
    // clipped to three lines by the overlay painter.
    let chat_history: HWND = CreateWindowExW(
        0,
        edit_class.as_ptr(),
        empty_text.as_ptr(),
        WS_CHILD_STYLE
            | WS_VISIBLE_STYLE
            | WS_BORDER_STYLE
            | WS_VSCROLL_STYLE
            | ES_MULTILINE
            | ES_AUTOVSCROLL
            | ES_READONLY,
        12, 42, 456, 210, chat, null_mut(), null_mut(), null(),
    );
    if chat_history.is_null() {
        return Err("CreateWindowExW(chat history) failed".to_owned());
    }
    let chat_edit: HWND = CreateWindowExW(
        0,
        edit_class.as_ptr(),
        empty_text.as_ptr(),
        WS_CHILD_STYLE
            | WS_VISIBLE_STYLE
            | WS_BORDER_STYLE
            | WS_VSCROLL_STYLE
            | ES_MULTILINE
            | ES_AUTOVSCROLL
            | ES_WANTRETURN,
        12, 280, 360, 80, chat, null_mut(), null_mut(), null(),
    );
    if chat_edit.is_null() {
        return Err("CreateWindowExW(chat edit) failed".to_owned());
    }
    ShowWindow(chat, SW_HIDE);

    let mut last_game_rect = zero_rect();
    let mut last_panel_rect = zero_rect();
    let mut last_guide_rect = zero_rect();
    let mut prev_left_down = false;
    let mut pointer_mode: Option<PanelPointerMode> = None;
    let mut live_panel_rect: Option<RECT> = None;
    let mut panel_ui = PanelUiState::default();
    let initial_overlay_config = state.config().overlay;
    let mut art_cache = CardArtCache::new(&initial_overlay_config.card_art_dirs);
    let mut chat_open = false;
    let mut last_chat_rect = zero_rect();
    let mut last_chat_render_key = String::new();
    let mut last_chat_transcript_key = String::new();

    loop {
        pump_window_messages();
        let config = state.config().overlay;
        if !config.enabled {
            pointer_mode = None;
            live_panel_rect = None;
            ShowWindow(highlight, SW_HIDE);
            ShowWindow(panel, SW_HIDE);
            ShowWindow(guide, SW_HIDE);
            ShowWindow(chat, SW_HIDE);
            chat_open = false;
            last_chat_render_key.clear();
            last_chat_transcript_key.clear();
            std::thread::sleep(Duration::from_millis(250));
            continue;
        }

        let game = find_hearthstone_window();
        if game.is_null() {
            pointer_mode = None;
            live_panel_rect = None;
            ShowWindow(highlight, SW_HIDE);
            ShowWindow(panel, SW_HIDE);
            ShowWindow(guide, SW_HIDE);
            ShowWindow(chat, SW_HIDE);
            chat_open = false;
            last_chat_render_key.clear();
            last_chat_transcript_key.clear();
            std::thread::sleep(Duration::from_millis(500));
            continue;
        }

        let foreground = GetForegroundWindow();
        if foreground != game
            && foreground != panel
            && foreground != guide
            && foreground != highlight
            && foreground != chat
        {
            pointer_mode = None;
            live_panel_rect = None;
            ShowWindow(highlight, SW_HIDE);
            ShowWindow(panel, SW_HIDE);
            ShowWindow(guide, SW_HIDE);
            ShowWindow(chat, SW_HIDE);
            chat_open = false;
            last_chat_render_key.clear();
            last_chat_transcript_key.clear();
            std::thread::sleep(Duration::from_millis(120));
            continue;
        }

        let Some(game_rect) = hearthstone_client_rect(game) else {
            std::thread::sleep(Duration::from_millis(120));
            continue;
        };
        let game_width = (game_rect.right - game_rect.left).max(1);
        let game_height = (game_rect.bottom - game_rect.top).max(1);

        if !same_rect(&game_rect, &last_game_rect) {
            if pointer_mode.is_some() {
                pointer_mode = None;
                live_panel_rect = None;
            }
            SetWindowPos(
                highlight, HWND_TOPMOST, game_rect.left, game_rect.top, game_width, game_height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            last_game_rect = game_rect;
        }

        let public = state.public_state();
        if !public.match_active {
            pointer_mode = None;
            live_panel_rect = None;
            ShowWindow(highlight, SW_HIDE);
            ShowWindow(panel, SW_HIDE);
            ShowWindow(guide, SW_HIDE);
            ShowWindow(chat, SW_HIDE);
            chat_open = false;
            last_chat_render_key.clear();
            last_chat_transcript_key.clear();
            std::thread::sleep(Duration::from_millis(120));
            continue;
        }
        panel_ui.sync_selection(public.selected_composition.as_ref().map(|item| item.id.as_str()));

        let left_down = (GetAsyncKeyState(VK_LBUTTON) as u16 & 0x8000) != 0;
        let mut cursor = POINT { x: 0, y: 0 };
        let has_cursor = GetCursorPos(&mut cursor) != 0;

        // While dragging/resizing, keep a live pixel-space rectangle for smooth
        // feedback. Ratios are written to config only when the mouse is released.
        if left_down && has_cursor {
            if let Some(mode) = pointer_mode {
                live_panel_rect = Some(match mode {
                    PanelPointerMode::Drag { grab_x, grab_y } => {
                        let current = live_panel_rect
                            .as_ref()
                            .map(copy_rect)
                            .unwrap_or_else(|| configured_panel_rect(&game_rect, &config));
                        let width = current.right - current.left;
                        let height = current.bottom - current.top;
                        clamp_panel_rect(
                            RECT {
                                left: cursor.x - grab_x,
                                top: cursor.y - grab_y,
                                right: cursor.x - grab_x + width,
                                bottom: cursor.y - grab_y + height,
                            },
                            &game_rect,
                        )
                    }
                    PanelPointerMode::Resize {
                        start_cursor_x,
                        start_cursor_y,
                        start_left,
                        start_top,
                        start_width,
                        start_height,
                    } => {
                        let (min_width, min_height, max_width, max_height) =
                            panel_size_bounds(game_width, game_height);
                        let width = (start_width + cursor.x - start_cursor_x)
                            .clamp(min_width, max_width);
                        let height = (start_height + cursor.y - start_cursor_y)
                            .clamp(min_height, max_height);
                        clamp_panel_rect(
                            RECT {
                                left: start_left,
                                top: start_top,
                                right: start_left + width,
                                bottom: start_top + height,
                            },
                            &game_rect,
                        )
                    }
                });
            }
        }

        let panel_rect = live_panel_rect
            .as_ref()
            .map(copy_rect)
            .unwrap_or_else(|| configured_panel_rect(&game_rect, &config));
        let panel_width = panel_rect.right - panel_rect.left;
        let panel_height = panel_rect.bottom - panel_rect.top;
        if !same_rect(&panel_rect, &last_panel_rect) {
            SetWindowPos(
                panel,
                HWND_TOPMOST,
                panel_rect.left,
                panel_rect.top,
                panel_width,
                panel_height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            last_panel_rect = panel_rect;
        } else {
            ShowWindow(panel, SW_SHOWNOACTIVATE);
        }
        SetLayeredWindowAttributes(panel, 0, config.panel_alpha, LWA_ALPHA);

        // Top composition guide: one horizontal row, centered over the game.
        let guide_width = ((game_width as f32 * config.guide_width_ratio) as i32).clamp(720, 1280);
        let guide_height = ((game_height as f32 * config.guide_height_ratio) as i32).clamp(104, 165);
        let guide_x = game_rect.left + (game_width - guide_width) / 2;
        let guide_y = game_rect.top + (game_height as f32 * config.guide_top_margin_ratio) as i32;
        let guide_rect = RECT {
            left: guide_x,
            top: guide_y,
            right: guide_x + guide_width,
            bottom: guide_y + guide_height,
        };
        if !same_rect(&guide_rect, &last_guide_rect) {
            SetWindowPos(
                guide, HWND_TOPMOST, guide_x, guide_y, guide_width, guide_height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            last_guide_rect = guide_rect;
        } else {
            ShowWindow(guide, SW_SHOWNOACTIVATE);
        }
        SetLayeredWindowAttributes(guide, 0, config.guide_alpha, LWA_ALPHA);

        let panel_hover = if has_cursor && panel_rect_contains(&panel_rect, cursor.x, cursor.y) {
            Some((cursor.x - panel_rect.left, cursor.y - panel_rect.top))
        } else {
            None
        };
        let guide_hover = if has_cursor && panel_rect_contains(&guide_rect, cursor.x, cursor.y) {
            Some((cursor.x - guide_rect.left, cursor.y - guide_rect.top))
        } else {
            None
        };

        let mut chat_rect = zero_rect();
        let mut chat_send_rect = UiRect { left: 0, top: 0, right: 0, bottom: 0 };
        let mut chat_close_rect = UiRect { left: 0, top: 0, right: 0, bottom: 0 };
        if chat_open {
            let chat_width = ((game_width as f32 * 0.44) as i32).clamp(520, 760).min(game_width);
            let chat_height = ((game_height as f32 * 0.60) as i32).clamp(420, 680).min(game_height);
            let preferred_left = panel_rect.left - chat_width - 12;
            let chat_left = preferred_left
                .max(game_rect.left)
                .min((game_rect.right - chat_width).max(game_rect.left));
            let chat_top = panel_rect
                .top
                .max(game_rect.top)
                .min((game_rect.bottom - chat_height).max(game_rect.top));
            chat_rect = RECT {
                left: chat_left,
                top: chat_top,
                right: chat_left + chat_width,
                bottom: chat_top + chat_height,
            };
            let rect_changed = !same_rect(&chat_rect, &last_chat_rect);
            if rect_changed {
                SetWindowPos(
                    chat,
                    HWND_TOPMOST,
                    chat_rect.left,
                    chat_rect.top,
                    chat_width,
                    chat_height,
                    SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
                last_chat_rect = copy_rect(&chat_rect);
            } else {
                ShowWindow(chat, SW_SHOW);
            }
            let edit_top = chat_height - 92;
            chat_send_rect = UiRect {
                left: chat_width - 82,
                top: edit_top,
                right: chat_width - 12,
                bottom: edit_top + 38,
            };
            chat_close_rect = UiRect {
                left: chat_width - 36,
                top: 5,
                right: chat_width - 8,
                bottom: 29,
            };

            // Crucial V0.4.6 fix: do not repaint the parent over the EDIT control
            // every overlay tick. Only repaint chat chrome/history when its own
            // state changes; then repaint the native child editor last.
            let chat_render_key = format!(
                "{}x{}|busy={}|status={}|messages={}",
                chat_width,
                chat_height,
                public.chat_busy,
                public.chat_status,
                serde_json::to_string(&public.chat_messages).unwrap_or_default(),
            );
            let chat_needs_redraw = rect_changed || chat_render_key != last_chat_render_key;
            if chat_needs_redraw {
                draw_chat_window(
                    chat,
                    chat_width,
                    chat_height,
                    &public,
                    chat_send_rect,
                    chat_close_rect,
                );
                last_chat_render_key = chat_render_key;
            }

            let history_top = 42;
            let history_bottom = (edit_top - 28).max(history_top + 80);
            SetWindowPos(
                chat_history,
                null_mut(),
                12,
                history_top,
                (chat_width - 24).max(220),
                (history_bottom - history_top).max(80),
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );

            let transcript_key = serde_json::to_string(&public.chat_messages).unwrap_or_default();
            if transcript_key != last_chat_transcript_key {
                let transcript = chat_transcript(&public.chat_messages);
                let transcript_w = wide(&transcript);
                SetWindowTextW(chat_history, transcript_w.as_ptr());
                let len = GetWindowTextLengthW(chat_history).max(0) as usize;
                SendMessageW(chat_history, EM_SETSEL, len, len as isize);
                SendMessageW(chat_history, EM_SCROLLCARET, 0, 0);
                last_chat_transcript_key = transcript_key;
            }

            SetWindowPos(
                chat_edit,
                null_mut(),
                12,
                edit_top,
                (chat_width - 106).max(160),
                72,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        } else {
            ShowWindow(chat, SW_HIDE);
            last_chat_render_key.clear();
        }

        let in_recruit = public.match_active && public.current_phase.eq_ignore_ascii_case("Recruit");
        let decision_revision_matches = public.tactical_plan.is_some()
            && public.decision_revision == public.shop_revision
            && public
                .tactical_plan
                .as_ref()
                .map(|plan| plan.shop_revision == public.shop_revision)
                .unwrap_or(false);
        let has_overlay_marks = if public.overlay_test_mode {
            true
        } else if public.tactical_plan.is_some() {
            decision_revision_matches && !public.decision_hits.is_empty()
        } else {
            !public.shop_hits.is_empty()
        };
        let marks_are_visually_settled = public.overlay_test_mode || public.overlay_marks_ready;
        if in_recruit && !public.current_shop.is_empty() && has_overlay_marks && marks_are_visually_settled {
            SetWindowPos(
                highlight, HWND_TOPMOST, game_rect.left, game_rect.top, game_width, game_height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            ShowWindow(highlight, SW_SHOWNOACTIVATE);
            draw_highlights(highlight, game_width, game_height, &public, &config);
        } else {
            clear_highlight(highlight, game_width, game_height);
            ShowWindow(highlight, SW_HIDE);
        }

        let panel_hits = draw_compact_decision_panel(
            panel,
            panel_width,
            panel_height,
            &public,
            panel_hover,
        );
        let guide_hits = draw_top_guide(
            guide,
            guide_width,
            guide_height,
            &public,
            &mut panel_ui,
            &config,
            &mut art_cache,
            guide_hover,
        );

        if left_down && !prev_left_down && has_cursor {
            if chat_open && panel_rect_contains(&chat_rect, cursor.x, cursor.y) {
                let local_x = cursor.x - chat_rect.left;
                let local_y = cursor.y - chat_rect.top;
                if chat_close_rect.contains(local_x, local_y) {
                    chat_open = false;
                    ShowWindow(chat, SW_HIDE);
                    last_chat_rect = zero_rect();
                    last_chat_render_key.clear();
                    last_chat_transcript_key.clear();
                    SetForegroundWindow(game);
                } else if chat_send_rect.contains(local_x, local_y) {
                    if !public.chat_busy {
                        let text_len = GetWindowTextLengthW(chat_edit).max(0) as usize;
                        let mut buffer = vec![0u16; text_len.saturating_add(1)];
                        let copied = GetWindowTextW(
                            chat_edit,
                            buffer.as_mut_ptr(),
                            buffer.len().min(i32::MAX as usize) as i32,
                        );
                        let text = if copied > 0 {
                            String::from_utf16_lossy(&buffer[..copied as usize])
                        } else {
                            String::new()
                        };
                        if !text.trim().is_empty() {
                            if let Err(error) = state.request_chat(&text) {
                                eprintln!("[overlay] chat: {error}");
                            } else {
                                let empty = wide("");
                                SetWindowTextW(chat_edit, empty.as_ptr());
                            }
                        }
                    }
                    SetFocus(chat_edit);
                }
            } else if panel_rect_contains(&panel_rect, cursor.x, cursor.y) {
                let local_x = cursor.x - panel_rect.left;
                let local_y = cursor.y - panel_rect.top;
                let resize_rect = panel_resize_handle(panel_width, panel_height);
                if resize_rect.contains(local_x, local_y) {
                    pointer_mode = Some(PanelPointerMode::Resize {
                        start_cursor_x: cursor.x,
                        start_cursor_y: cursor.y,
                        start_left: panel_rect.left,
                        start_top: panel_rect.top,
                        start_width: panel_width,
                        start_height: panel_height,
                    });
                    live_panel_rect = Some(panel_rect);
                } else if let Some(region) = panel_hits
                    .iter()
                    .find(|region| region.rect.contains(local_x, local_y))
                    .cloned()
                {
                    let reset_layout = matches!(&region.action, &PanelAction::ResetPanelLayout);
                    if matches!(&region.action, &PanelAction::OpenChat) {
                        chat_open = true;
                        ShowWindow(chat, SW_SHOW);
                        SetForegroundWindow(chat);
                        SetFocus(chat_edit);
                    } else {
                        handle_panel_action(&state, &mut panel_ui, region.action);
                    }
                    if reset_layout {
                        live_panel_rect = None;
                        last_panel_rect = zero_rect();
                    }
                } else if local_y < 30 {
                    pointer_mode = Some(PanelPointerMode::Drag {
                        grab_x: cursor.x - panel_rect.left,
                        grab_y: cursor.y - panel_rect.top,
                    });
                    live_panel_rect = Some(panel_rect);
                }
            } else if panel_rect_contains(&guide_rect, cursor.x, cursor.y) {
                let local_x = cursor.x - guide_rect.left;
                let local_y = cursor.y - guide_rect.top;
                if let Some(region) = guide_hits
                    .iter()
                    .find(|region| region.rect.contains(local_x, local_y))
                    .cloned()
                {
                    handle_panel_action(&state, &mut panel_ui, region.action);
                }
            }
        }

        if !left_down && prev_left_down && pointer_mode.take().is_some() {
            let final_rect = live_panel_rect
                .as_ref()
                .map(copy_rect)
                .unwrap_or_else(|| copy_rect(&panel_rect));
            let x_ratio = (final_rect.left - game_rect.left) as f32 / game_width as f32;
            let y_ratio = (final_rect.top - game_rect.top) as f32 / game_height as f32;
            let width_ratio = (final_rect.right - final_rect.left) as f32 / game_width as f32;
            let height_ratio = (final_rect.bottom - final_rect.top) as f32 / game_height as f32;
            if let Err(error) = state.update_overlay_panel_layout(
                x_ratio,
                y_ratio,
                width_ratio,
                height_ratio,
            ) {
                eprintln!("[overlay] save panel layout: {error}");
            }
            live_panel_rect = None;
            last_panel_rect = zero_rect();
        }

        prev_left_down = left_down;
        std::thread::sleep(Duration::from_millis(if pointer_mode.is_some() {
            16
        } else if chat_open {
            45
        } else {
            90
        }));
    }

}

#[cfg(windows)]
fn copy_rect(rect: &windows_sys::Win32::Foundation::RECT) -> windows_sys::Win32::Foundation::RECT {
    windows_sys::Win32::Foundation::RECT {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    }
}

#[cfg(windows)]
fn panel_size_bounds(game_width: i32, game_height: i32) -> (i32, i32, i32, i32) {
    let min_width = 285.min(game_width.max(1));
    let min_height = 190.min(game_height.max(1));
    let max_width = ((game_width as f32 * 0.62) as i32)
        .max(min_width)
        .min(game_width.max(min_width));
    let max_height = ((game_height as f32 * 0.78) as i32)
        .max(min_height)
        .min(game_height.max(min_height));
    (min_width, min_height, max_width, max_height)
}

#[cfg(windows)]
fn configured_panel_rect(
    game_rect: &windows_sys::Win32::Foundation::RECT,
    config: &OverlayConfig,
) -> windows_sys::Win32::Foundation::RECT {
    use windows_sys::Win32::Foundation::RECT;

    let game_width = (game_rect.right - game_rect.left).max(1);
    let game_height = (game_rect.bottom - game_rect.top).max(1);
    let (min_width, min_height, max_width, max_height) = panel_size_bounds(game_width, game_height);
    let width = ((game_width as f32 * config.panel_width_ratio) as i32)
        .clamp(min_width, max_width);
    let height = ((game_height as f32 * config.panel_height_ratio) as i32)
        .clamp(min_height, max_height);

    let fallback_x = game_rect.right
        - (game_width as f32 * config.panel_right_margin_ratio) as i32
        - width;
    let fallback_y = game_rect.bottom
        - (game_height as f32 * config.panel_bottom_margin_ratio) as i32
        - height;
    let left = config
        .panel_x_ratio
        .map(|ratio| game_rect.left + (ratio.clamp(0.0, 1.0) * game_width as f32) as i32)
        .unwrap_or(fallback_x);
    let top = config
        .panel_y_ratio
        .map(|ratio| game_rect.top + (ratio.clamp(0.0, 1.0) * game_height as f32) as i32)
        .unwrap_or(fallback_y);

    clamp_panel_rect(
        RECT {
            left,
            top,
            right: left + width,
            bottom: top + height,
        },
        game_rect,
    )
}

#[cfg(windows)]
fn clamp_panel_rect(
    rect: windows_sys::Win32::Foundation::RECT,
    game_rect: &windows_sys::Win32::Foundation::RECT,
) -> windows_sys::Win32::Foundation::RECT {
    use windows_sys::Win32::Foundation::RECT;

    let width = (rect.right - rect.left).max(1);
    let height = (rect.bottom - rect.top).max(1);
    let max_left = (game_rect.right - width).max(game_rect.left);
    let max_top = (game_rect.bottom - height).max(game_rect.top);
    let left = rect.left.clamp(game_rect.left, max_left);
    let top = rect.top.clamp(game_rect.top, max_top);
    RECT {
        left,
        top,
        right: left + width,
        bottom: top + height,
    }
}

#[cfg(windows)]
fn panel_resize_handle(width: i32, height: i32) -> UiRect {
    UiRect {
        left: (width - 24).max(0),
        top: (height - 24).max(0),
        right: width,
        bottom: height,
    }
}

#[cfg(windows)]
#[derive(Debug, Clone)]
struct CardArt {
    width: u32,
    height: u32,
    bgra: Vec<u8>,
}

#[cfg(windows)]
struct CardArtCache {
    entries: std::collections::HashMap<String, Option<CardArt>>,
    roots: Vec<std::path::PathBuf>,
}

#[cfg(windows)]
impl CardArtCache {
    fn new(extra_roots: &[std::path::PathBuf]) -> Self {
        // V0.5.0.6: prefer HDT's JPG portrait/tile caches over full-card PNGs.
        // Some historical PNG assets contain a broken iCCP profile. The profile
        // is harmless, but native libpng builds may print
        // "iCCP: known incorrect sRGB profile". JPG portraits avoid that path
        // entirely and are also a better fit for the compact horizontal guide.
        let mut roots = Vec::new();
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let base = std::path::PathBuf::from(local);
            roots.push(base.join("HearthCoach").join("card_art"));
            push_hdt_art_roots(&mut roots, &base.join("HearthstoneDeckTracker"));
        }
        if let Ok(app) = std::env::var("APPDATA") {
            push_hdt_art_roots(
                &mut roots,
                &std::path::PathBuf::from(app).join("HearthstoneDeckTracker"),
            );
        }
        roots.extend(extra_roots.iter().cloned());

        let mut unique = Vec::new();
        for root in roots {
            if !unique.iter().any(|old: &std::path::PathBuf| old == &root) {
                unique.push(root);
            }
        }
        Self { entries: std::collections::HashMap::new(), roots: unique }
    }

    fn get(&mut self, card_id: &str) -> Option<&CardArt> {
        if !self.entries.contains_key(card_id) {
            let art = self.load(card_id);
            self.entries.insert(card_id.to_owned(), art);
        }
        self.entries.get(card_id).and_then(|art| art.as_ref())
    }

    fn load(&self, card_id: &str) -> Option<CardArt> {
        // Do not let one damaged/stale cache file suppress the fallback text or
        // a healthy image from another cache root. Try every candidate in a
        // deterministic preference order.
        for path in self.find_art_paths(card_id) {
            if let Some(art) = load_card_art_file(&path) {
                return Some(art);
            }
        }
        None
    }

    fn find_art_paths(&self, card_id: &str) -> Vec<std::path::PathBuf> {
        let mut candidates = Vec::new();
        for root in &self.roots {
            collect_card_art_recursively(root, card_id, 4, &mut candidates);
        }
        candidates.sort_by_key(|path| card_art_preference(path));
        candidates.dedup();
        candidates
    }
}

#[cfg(windows)]
fn push_hdt_art_roots(roots: &mut Vec<std::path::PathBuf>, hdt: &std::path::Path) {
    // Current HDT stores portraits/tiles as JPG and full card renders as PNG.
    // Prefer the metadata-light JPG assets, then fall back to full renders.
    roots.push(hdt.join("Images").join("CardPortraits"));
    roots.push(hdt.join("Images").join("CardTiles"));
    roots.push(hdt.join("Images").join("CardImages"));
    // Older/local layouts occasionally use these names directly.
    roots.push(hdt.join("CardPortraits"));
    roots.push(hdt.join("CardTiles"));
    roots.push(hdt.join("CardImages"));
}

#[cfg(windows)]
fn card_art_preference(path: &std::path::Path) -> (u8, u8, String) {
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let ext_rank = match ext.as_str() {
        "jpg" | "jpeg" => 0,
        "webp" => 1,
        "png" => 2,
        _ => 3,
    };
    let lower = path.to_string_lossy().to_ascii_lowercase();
    let folder_rank = if lower.contains("cardportraits") {
        0
    } else if lower.contains("cardtiles") {
        1
    } else if lower.contains("cardimages") {
        2
    } else {
        3
    };
    (ext_rank, folder_rank, lower)
}

#[cfg(windows)]
fn load_card_art_file(path: &std::path::Path) -> Option<CardArt> {
    let mut bytes = std::fs::read(path).ok()?;
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    // Strip the optional ICC profile chunk before decoding PNGs. The compact
    // guide does not need color-management metadata and this makes malformed
    // iCCP chunks non-fatal/silent across machines and decoder versions.
    if ext == "png" {
        bytes = strip_png_iccp_chunk(&bytes).unwrap_or(bytes);
    }

    // `image` uses Rust decoders here; no native libpng dependency is needed.
    let format = image::guess_format(&bytes).ok()?;
    let rgba = image::load_from_memory_with_format(&bytes, format).ok()?.to_rgba8();
    let (width, height) = rgba.dimensions();
    if width < 32 || height < 32 {
        return None;
    }
    let aspect = width as f32 / height as f32;
    if !(0.35..=1.60).contains(&aspect) {
        return None;
    }
    let mut bgra = rgba.into_raw();
    for pixel in bgra.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Some(CardArt { width, height, bgra })
}

#[cfg(windows)]
fn strip_png_iccp_chunk(bytes: &[u8]) -> Option<Vec<u8>> {
    const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1a\n";
    if bytes.len() < PNG_SIGNATURE.len() || &bytes[..8] != PNG_SIGNATURE {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len());
    out.extend_from_slice(PNG_SIGNATURE);
    let mut offset = 8usize;
    let mut removed = false;
    while offset + 12 <= bytes.len() {
        let len = u32::from_be_bytes(bytes[offset..offset + 4].try_into().ok()?) as usize;
        let chunk_end = offset.checked_add(12)?.checked_add(len)?;
        if chunk_end > bytes.len() {
            return None;
        }
        let chunk_type = &bytes[offset + 4..offset + 8];
        if chunk_type == b"iCCP" {
            removed = true;
        } else {
            out.extend_from_slice(&bytes[offset..chunk_end]);
        }
        offset = chunk_end;
        if chunk_type == b"IEND" {
            break;
        }
    }
    if offset < bytes.len() {
        out.extend_from_slice(&bytes[offset..]);
    }
    removed.then_some(out)
}

#[cfg(windows)]
fn collect_card_art_recursively(
    root: &std::path::Path,
    card_id: &str,
    depth: usize,
    out: &mut Vec<std::path::PathBuf>,
) {
    if depth == 0 || !root.exists() {
        return;
    }
    let target = card_id.to_ascii_lowercase();
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            let stem = path.file_stem().and_then(|value| value.to_str()).unwrap_or("");
            let ext = path.extension().and_then(|value| value.to_str()).unwrap_or("");
            let stem_lower = stem.to_ascii_lowercase();
            let suffix_ok = stem_lower
                .strip_prefix(&format!("{target}_"))
                .map(|suffix| {
                    suffix.chars().all(|ch| ch.is_ascii_digit())
                        || suffix.starts_with("portrait")
                        || suffix.starts_with("card")
                        || suffix.starts_with("full")
                        || suffix.starts_with("triple")
                })
                .unwrap_or(false);
            if (stem_lower == target || suffix_ok)
                && matches!(ext.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "webp")
            {
                out.push(path);
            }
        } else if path.is_dir() {
            collect_card_art_recursively(&path, card_id, depth - 1, out);
        }
    }
}

#[cfg(windows)]
unsafe fn draw_card_art(
    dc: windows_sys::Win32::Graphics::Gdi::HDC,
    art: &CardArt,
    rect: UiRect,
) {
    use std::{ffi::c_void, mem};
    use windows_sys::Win32::Graphics::Gdi::{
        StretchDIBits, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, SRCCOPY,
    };
    let mut info: BITMAPINFO = mem::zeroed();
    info.bmiHeader = BITMAPINFOHEADER {
        biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: art.width as i32,
        biHeight: -(art.height as i32),
        biPlanes: 1,
        biBitCount: 32,
        biCompression: 0,
        biSizeImage: 0,
        biXPelsPerMeter: 0,
        biYPelsPerMeter: 0,
        biClrUsed: 0,
        biClrImportant: 0,
    };
    let dst_w = (rect.right - rect.left).max(1);
    let dst_h = (rect.bottom - rect.top).max(1);
    let dst_ratio = dst_w as f32 / dst_h as f32;
    let src_ratio = art.width as f32 / art.height as f32;
    let (src_x, src_y, src_w, src_h) = if src_ratio > dst_ratio {
        let crop_w = (art.height as f32 * dst_ratio).round().max(1.0) as i32;
        (((art.width as i32 - crop_w) / 2).max(0), 0, crop_w, art.height as i32)
    } else {
        let crop_h = (art.width as f32 / dst_ratio).round().max(1.0) as i32;
        (0, ((art.height as i32 - crop_h) / 2).max(0), art.width as i32, crop_h)
    };
    StretchDIBits(
        dc,
        rect.left,
        rect.top,
        dst_w,
        dst_h,
        src_x,
        src_y,
        src_w,
        src_h,
        art.bgra.as_ptr().cast::<c_void>(),
        &info,
        DIB_RGB_COLORS,
        SRCCOPY,
    );
}

#[cfg(windows)]
unsafe fn clear_highlight(
    hwnd: windows_sys::Win32::Foundation::HWND,
    width: i32,
    height: i32,
) {
    use windows_sys::Win32::Graphics::Gdi::{CreateSolidBrush, DeleteObject, FillRect, GetDC, ReleaseDC};
    let dc = GetDC(hwnd);
    if dc.is_null() {
        return;
    }
    let rect = windows_sys::Win32::Foundation::RECT {
        left: 0,
        top: 0,
        right: width,
        bottom: height,
    };
    let brush = CreateSolidBrush(0);
    if !brush.is_null() {
        FillRect(dc, &rect, brush);
        DeleteObject(brush);
    }
    ReleaseDC(hwnd, dc);
}

#[cfg(windows)]
unsafe fn draw_highlights(
    hwnd: windows_sys::Win32::Foundation::HWND,
    width: i32,
    height: i32,
    public: &super::server::PublicState,
    config: &OverlayConfig,
) {
    use windows_sys::Win32::Graphics::Gdi::{GetDC, ReleaseDC, SetBkMode, TRANSPARENT};

    clear_highlight(hwnd, width, height);
    let dc = GetDC(hwnd);
    if dc.is_null() {
        return;
    }
    SetBkMode(dc, TRANSPARENT as i32);

    let count = public.current_shop.len().max(1);
    let center_x = width as f32 * config.shop_center_x_ratio;
    let spacing = width as f32 * config.shop_slot_spacing_ratio;
    let card_width = (width as f32 * config.card_width_ratio) as i32;
    let card_height = (height as f32 * config.card_height_ratio) as i32;
    let top = (height as f32 * config.shop_top_ratio) as i32;
    let first_center = center_x - spacing * (count.saturating_sub(1) as f32) / 2.0;

    if public.overlay_test_mode {
        let mut test_indices = vec![0usize];
        if count >= 3 {
            test_indices.push(2);
        } else if count >= 2 {
            test_indices.push(1);
        }
        test_indices.sort_unstable();
        test_indices.dedup();
        for (order, shop_index) in test_indices.into_iter().enumerate() {
            draw_one_highlight(
                dc,
                shop_index,
                count,
                first_center,
                spacing,
                card_width,
                card_height,
                top,
                config.border_px.max(2),
                if order == 0 { "S" } else { "A" },
                None,
                "测试框",
            );
        }
    } else {
        let active_hits = if public.tactical_plan.is_some() {
            &public.decision_hits
        } else {
            &public.shop_hits
        };
        for hit in active_hits {
            if hit.shop_index >= count {
                continue;
            }
            draw_one_highlight(
                dc,
                hit.shop_index,
                count,
                first_center,
                spacing,
                card_width,
                card_height,
                top,
                config.border_px.max(2),
                &hit.priority,
                hit.tavern_tier,
                &hit.role,
            );
        }
    }

    ReleaseDC(hwnd, dc);
}

#[cfg(windows)]
unsafe fn draw_one_highlight(
    dc: windows_sys::Win32::Graphics::Gdi::HDC,
    shop_index: usize,
    count: usize,
    first_center: f32,
    spacing: f32,
    card_width: i32,
    card_height: i32,
    top: i32,
    border_px: i32,
    priority: &str,
    tavern_tier: Option<u8>,
    role: &str,
) {
    use windows_sys::Win32::Graphics::Gdi::{SetTextColor, TRANSPARENT, SetBkMode};
    if shop_index >= count {
        return;
    }
    let cx = first_center + spacing * shop_index as f32;
    let x1 = (cx - card_width as f32 / 2.0) as i32;
    let x2 = x1 + card_width;
    let y1 = top;
    let y2 = y1 + card_height;
    let color = priority_color(priority);

    // Four narrow strips keep the actual Hearthstone card fully visible.
    draw_hollow_frame(dc, UiRect { left: x1, top: y1, right: x2, bottom: y2 }, border_px, color);
    SetBkMode(dc, TRANSPARENT as i32);
    SetTextColor(dc, color);
    let tier = tavern_tier.map(|tier| format!("{tier}本")).unwrap_or_else(|| "".to_owned());
    let label = if tier.is_empty() {
        format!("{} · {}", priority, role)
    } else {
        format!("{} · {} · {}", priority, tier, role)
    };
    text_out(dc, x1 + 2, (y1 - 20).max(0), &label);
}

#[cfg(windows)]
unsafe fn draw_compact_decision_panel(
    hwnd: windows_sys::Win32::Foundation::HWND,
    width: i32,
    height: i32,
    public: &super::server::PublicState,
    hover: Option<(i32, i32)>,
) -> Vec<HitRegion> {
    use windows_sys::Win32::{
        Foundation::RECT,
        Graphics::Gdi::{GetDC, ReleaseDC, SetBkMode, SetTextColor, TRANSPARENT},
    };
    let dc = GetDC(hwnd);
    if dc.is_null() {
        return Vec::new();
    }
    fill(
        dc,
        RECT { left: 0, top: 0, right: width, bottom: height },
        rgb(15, 18, 24),
    );
    SetBkMode(dc, TRANSPARENT as i32);
    let mut hits = Vec::new();
    let pad = 11;
    let content_width = (width - pad * 2 - 8).max(80);
    let mut y = 8;

    SetTextColor(dc, rgb(245, 247, 250));
    text_out(dc, pad, y, "HearthCoach");
    let reset_rect = UiRect {
        left: (width - 48).max(0),
        top: 4,
        right: (width - 7).max(1),
        bottom: 25,
    };
    let chat_rect = UiRect {
        left: (width - 109).max(76),
        top: 4,
        right: (width - 53).max(78),
        bottom: 25,
    };
    draw_button(dc, chat_rect, if public.chat_busy { "AI…" } else { "AI交流" }, rgb(48, 79, 102));
    hits.push(HitRegion { rect: chat_rect, action: PanelAction::OpenChat });
    draw_button(dc, reset_rect, "复位", rgb(48, 56, 67));
    hits.push(HitRegion {
        rect: reset_rect,
        action: PanelAction::ResetPanelLayout,
    });
    SetTextColor(dc, rgb(165, 178, 192));
    let round = if public.current_round == 0 { "-".to_owned() } else { public.current_round.to_string() };
    text_out(
        dc,
        (pad + 92).min((width - 122).max(pad)),
        y,
        &format!("R{round} · {}", phase_cn(&public.current_phase)),
    );
    y += 23;

    // Keep diagnostics to one compact line. Details remain available in logs/chat.
    if let Some(error) = public.error.as_deref() {
        SetTextColor(dc, rgb(255, 126, 126));
        let used = draw_wrapped_text(dc, pad, y, error, content_width, 2, 17);
        y += used.max(17) + 3;
    }

    // V0.4.5: goal is collapsed by default. Hovering this one-line strip shows
    // a temporary floating detail card without permanently growing the panel.
    let goal_rect = UiRect {
        left: pad,
        top: y,
        right: width - pad,
        bottom: y + 27,
    };
    fill_ui_rect(dc, goal_rect, rgb(28, 32, 39));
    draw_hollow_frame(dc, goal_rect, 1, rgb(99, 88, 53));
    let goal_hovered = hover
        .map(|(x, y)| goal_rect.contains(x, y))
        .unwrap_or(false);
    if let Some(plan) = public.round_plan.as_ref() {
        SetTextColor(dc, rgb(245, 211, 116));
        let arrow = if goal_hovered { "▾" } else { "▸" };
        let goal_text = format!("{arrow} 小目标：{}", plan.primary_goal);
        text_out(
            dc,
            goal_rect.left + 6,
            goal_rect.top + 5,
            &truncate_text_pixels(dc, &goal_text, (goal_rect.right - goal_rect.left - 12).max(60)),
        );
    } else {
        SetTextColor(dc, rgb(190, 199, 209));
        text_out(dc, goal_rect.left + 6, goal_rect.top + 5, "▸ 小目标：等待 Combat Planner…");
    }
    y = goal_rect.bottom + 7;

    draw_separator(dc, pad, width - pad, y);
    y += 7;

    let in_trinket = public.active_choice_kind.as_ref() == Some(&crate::harness::ChoiceKind::Trinket);
    if in_trinket {
        SetTextColor(dc, rgb(210, 167, 255));
        let status = if public.trinket_ranking_busy {
            format!("饰品规划 · {} 个候选 · 排序中…", public.active_choice_option_count)
        } else {
            format!("饰品规划 · {} 个候选", public.active_choice_option_count)
        };
        text_out(dc, pad, y, &truncate_text_pixels(dc, &status, content_width));
        y += 20;

        if !public.trinket_rankings.is_empty() {
            for (index, item) in public.trinket_rankings.iter().take(3).enumerate() {
                if y + 34 >= height - 8 { break; }
                SetTextColor(dc, if index == 0 { rgb(229, 194, 255) } else { rgb(205, 190, 220) });
                let label = format!("{}. {:.1} · {}", index + 1, item.score, item.name);
                text_out(dc, pad + 4, y, &truncate_text_pixels(dc, &label, content_width - 4));
                y += 17;
                SetTextColor(dc, rgb(171, 157, 190));
                let detail = if item.role.is_empty() {
                    item.reason.clone()
                } else {
                    format!("{} · {}", item.role, item.reason)
                };
                let used = draw_wrapped_text(dc, pad + 12, y, &detail, content_width - 12, 1, 16);
                y += used.max(16) + 2;
            }
        } else if !public.active_choice_options.is_empty() {
            // Even before AI ranking finishes, surface the real candidate names.
            for (index, option) in public.active_choice_options.iter().take(4).enumerate() {
                if y + 18 >= height - 8 { break; }
                SetTextColor(dc, rgb(205, 190, 220));
                let label = format!("{}. {}", index + 1, option.name);
                text_out(dc, pad + 4, y, &truncate_text_pixels(dc, &label, content_width - 4));
                y += 18;
            }
        } else if let Some(trinket) = public.trinket_plan.as_ref() {
            SetTextColor(dc, rgb(220, 207, 237));
            let _ = draw_wrapped_text(
                dc,
                pad + 4,
                y,
                &trinket.desired_effect,
                content_width - 4,
                2,
                17,
            );
        } else {
            SetTextColor(dc, rgb(190, 181, 204));
            text_out(dc, pad + 4, y, "正在读取实际饰品候选…");
        }
    } else {
        SetTextColor(dc, rgb(139, 222, 158));
        text_out(dc, pad, y, "当前动作");
        y += 19;
        if let Some(tactical) = public.tactical_plan.as_ref() {
            for (index, action) in tactical.ranked_actions.iter().take(2).enumerate() {
                if y + 18 >= height - 30 { break; }
                SetTextColor(dc, if index == 0 { rgb(145, 235, 165) } else { rgb(195, 215, 165) });
                let label = format!("{}. {:.1} · {}", index + 1, action.score, action.label);
                let used = draw_wrapped_text(
                    dc,
                    pad + 4,
                    y,
                    &label,
                    content_width - 4,
                    2,
                    17,
                );
                y += used.max(17) + 2;
            }
            if !tactical.route.is_empty() && y + 17 < height - 5 {
                let route = tactical.route.iter().map(|action| action.label.as_str()).collect::<Vec<_>>().join(" → ");
                SetTextColor(dc, rgb(154, 184, 211));
                let _ = draw_wrapped_text(
                    dc,
                    pad + 4,
                    y,
                    &format!("路线：{route}"),
                    content_width - 4,
                    2,
                    17,
                );
            }
        } else {
            SetTextColor(dc, rgb(170, 181, 194));
            text_out(dc, pad + 4, y, "等待稳定商店状态…");
        }
    }

    if goal_hovered {
        if let Some(plan) = public.round_plan.as_ref() {
            draw_goal_hover_card(dc, width, height, goal_rect, plan);
        }
    }

    SetTextColor(dc, rgb(102, 116, 132));
    text_out(dc, (width - 19).max(0), (height - 22).max(0), "↘");
    ReleaseDC(hwnd, dc);
    hits
}

#[cfg(windows)]
unsafe fn draw_goal_hover_card(
    dc: windows_sys::Win32::Graphics::Gdi::HDC,
    width: i32,
    height: i32,
    goal_rect: UiRect,
    plan: &super::model::RoundPlan,
) {
    use windows_sys::Win32::Graphics::Gdi::SetTextColor;

    // Hover details temporarily take over the panel's content area instead of
    // trying to squeeze an expanding tooltip between the compact action lines.
    // This preserves the small default panel while guaranteeing every rendered
    // line stays inside its actual pixel width.
    let rect = UiRect {
        left: 6,
        top: goal_rect.top,
        right: (width - 6).max(70),
        bottom: (height - 26).max(goal_rect.bottom + 58),
    };
    fill_ui_rect(dc, rect, rgb(20, 23, 29));
    draw_hollow_frame(dc, rect, 2, rgb(180, 149, 66));
    let text_width = (rect.right - rect.left - 16).max(70);
    let mut y = rect.top + 7;

    SetTextColor(dc, rgb(250, 218, 126));
    text_out(dc, rect.left + 8, y, &format!("第 {} 回合小目标", plan.target_round));
    y += 20;

    SetTextColor(dc, rgb(235, 229, 207));
    let used = draw_wrapped_text(
        dc,
        rect.left + 8,
        y,
        &plan.primary_goal,
        text_width,
        4,
        17,
    );
    y += used.max(17) + 3;

    if !plan.secondary_goals.is_empty() && y + 18 < rect.bottom {
        SetTextColor(dc, rgb(177, 190, 204));
        let secondary = format!("次要：{}", plan.secondary_goals.iter().take(2).map(String::as_str).collect::<Vec<_>>().join(" / "));
        let remaining = ((rect.bottom - y - 38) / 16).clamp(1, 2) as usize;
        let used = draw_wrapped_text(dc, rect.left + 8, y, &secondary, text_width, remaining, 16);
        y += used.max(16) + 2;
    }

    if y + 17 < rect.bottom {
        SetTextColor(dc, rgb(159, 182, 202));
        let upgrade = format!(
            "升本：{}{}",
            upgrade_posture_cn(plan.tier_policy.posture),
            plan.tier_policy.target_tier.map(|tier| format!(" · 目标T{tier}")).unwrap_or_default()
        );
        let used = draw_wrapped_text(dc, rect.left + 8, y, &upgrade, text_width, 2, 16);
        y += used.max(16) + 2;
    }

    if let Some(condition) = plan.replan_conditions.first() {
        if y + 16 < rect.bottom {
            SetTextColor(dc, rgb(138, 154, 171));
            let _ = draw_wrapped_text(
                dc,
                rect.left + 8,
                y,
                &format!("重规划：{condition}"),
                text_width,
                2,
                16,
            );
        }
    }
}

#[cfg(windows)]
unsafe fn draw_top_guide(
    hwnd: windows_sys::Win32::Foundation::HWND,
    width: i32,
    height: i32,
    public: &super::server::PublicState,
    _ui: &mut PanelUiState,
    config: &OverlayConfig,
    art_cache: &mut CardArtCache,
    hover: Option<(i32, i32)>,
) -> Vec<HitRegion> {
    use windows_sys::Win32::{
        Foundation::RECT,
        Graphics::Gdi::{GetDC, ReleaseDC, SetBkMode, SetTextColor, TRANSPARENT},
    };
    let dc = GetDC(hwnd);
    if dc.is_null() { return Vec::new(); }
    fill(dc, RECT { left: 0, top: 0, right: width, bottom: height }, rgb(15, 18, 24));
    SetBkMode(dc, TRANSPARENT as i32);
    let mut hits = Vec::new();
    let pad = 10;
    let header_h = 28;

    SetTextColor(dc, rgb(244, 247, 250));
    text_out(dc, pad, 6, "阵容指南");

    let Some(selected) = public.selected_composition.as_ref() else {
        if public.compositions.is_empty() {
            let status = if public.available_tribes.is_empty() {
                "等待本局可用种族信息…".to_owned()
            } else if !public.api_key_configured {
                "请先在 Control Center 配置模型/API Key".to_owned()
            } else if public.ai_status.starts_with("AI 正在")
                || public.ai_status.contains("正在自动准备阵容指南")
            {
                public.ai_status.clone()
            } else {
                "阵容指南尚未生成".to_owned()
            };
            SetTextColor(dc, rgb(170, 181, 194));
            text_out(dc, 88, 6, &truncate_chars(&status, 52));
            SetTextColor(dc, rgb(139, 151, 164));
            text_out(dc, pad, header_h + 18, &truncate_chars(&public.ai_status, 90));
            if public.api_key_configured
                && !public.available_tribes.is_empty()
                && !public.ai_status.starts_with("AI 正在")
            {
                let rect = UiRect { left: width - 165, top: 3, right: width - 10, bottom: 26 };
                draw_button(dc, rect, "重新分析阵容", rgb(41, 119, 184));
                hits.push(HitRegion { rect, action: PanelAction::Analyze });
            }
        } else {
            SetTextColor(dc, rgb(155, 177, 198));
            text_out(dc, 88, 6, "选择阵容：");
            let start_x = 170;
            let gap = 7;
            let count = public.compositions.len().min(4);
            let box_w = ((width - start_x - pad - gap * (count.saturating_sub(1) as i32)) / count.max(1) as i32).max(120);
            for (idx, comp) in public.compositions.iter().take(count).enumerate() {
                let left = start_x + idx as i32 * (box_w + gap);
                let rect = UiRect { left, top: 2, right: (left + box_w).min(width - pad), bottom: 27 };
                draw_button(dc, rect, &truncate_chars(&comp.name, 12), rgb(47, 68, 86));
                hits.push(HitRegion { rect, action: PanelAction::SelectComposition(comp.id.clone()) });
            }
            SetTextColor(dc, rgb(139, 151, 164));
            text_out(dc, pad, header_h + 18, "请选择一个阵容方向；也可等待自动指南完成。");
        }
        ReleaseDC(hwnd, dc);
        return hits;
    };

    SetTextColor(dc, rgb(124, 203, 255));
    text_out(dc, 88, 6, &truncate_chars(&selected.name, 20));
    let change_rect = UiRect { left: width - 72, top: 3, right: width - 10, bottom: 26 };
    draw_button(dc, change_rect, "换阵容", rgb(57, 68, 80));
    hits.push(HitRegion { rect: change_rect, action: PanelAction::ResetComposition });

    let Some(watchlist) = public.watchlist.as_ref() else {
        SetTextColor(dc, rgb(175, 185, 196));
        text_out(dc, 260, 6, &truncate_chars(&public.ai_status, 55));
        SetTextColor(dc, rgb(139, 151, 164));
        text_out(dc, pad, header_h + 18, "阵容方向已选择，正在生成前/中/后期卡牌指南…");
        ReleaseDC(hwnd, dc);
        return hits;
    };

    // The top guide is intentionally automatic. Round -> current_stage is the
    // single source of truth, so entering mid/late game immediately swaps the
    // horizontal watchlist without requiring a click.
    let stage = public.current_stage.as_str();
    SetTextColor(dc, rgb(142, 182, 219));
    text_out(
        dc,
        (width - 190).max(180),
        6,
        &format!("自动阶段 · {}", stage_cn(stage)),
    );

    let mut cards = watchlist
        .stages
        .iter()
        .find(|item| item.stage == stage)
        .map(|item| item.cards.clone())
        .unwrap_or_default();
    cards.sort_by_key(|card| (priority_rank(&card.priority), card.tavern_tier.unwrap_or(9), card.name.clone()));
    let limit = config.guide_card_limit.clamp(3, 9).min(cards.len());
    if limit == 0 {
        SetTextColor(dc, rgb(170, 181, 194));
        text_out(dc, pad, header_h + 18, "当前阶段没有阵容指南卡牌");
        ReleaseDC(hwnd, dc);
        return hits;
    }

    let gap = 6;
    let content_top = header_h + 2;
    let tile_w = ((width - pad * 2 - gap * (limit.saturating_sub(1) as i32)) / limit as i32).max(90);
    let mut hovered_card: Option<(super::model::WatchCard, UiRect)> = None;
    for (idx, card) in cards.iter().take(limit).enumerate() {
        let left = pad + idx as i32 * (tile_w + gap);
        let tile = UiRect { left, top: content_top, right: (left + tile_w).min(width - pad), bottom: height - 6 };
        fill_ui_rect(dc, tile, rgb(26, 31, 39));
        let is_hovered = hover
            .map(|(x, y)| tile.contains(x, y))
            .unwrap_or(false);
        draw_hollow_frame(
            dc,
            tile,
            if is_hovered { 2 } else { 1 },
            priority_color(&card.priority),
        );
        if is_hovered {
            hovered_card = Some((card.clone(), tile));
        }

        let art_w = ((tile.right - tile.left) as f32 * 0.38).clamp(34.0, 58.0) as i32;
        let art_rect = UiRect {
            left: tile.left + 4,
            top: tile.top + 4,
            right: tile.left + 4 + art_w,
            bottom: tile.bottom - 4,
        };
        if let Some(art) = art_cache.get(&card.card_id) {
            draw_card_art(dc, art, art_rect);
        } else {
            fill_ui_rect(dc, art_rect, rgb(38, 45, 55));
            SetTextColor(dc, priority_color(&card.priority));
            text_out(dc, art_rect.left + 7, art_rect.top + 7, &card.priority);
            SetTextColor(dc, rgb(180, 190, 201));
            text_out(dc, art_rect.left + 7, art_rect.top + 27, &card.tavern_tier.map(|t| format!("T{t}")).unwrap_or("T?".to_owned()));
        }

        let text_x = art_rect.right + 5;
        SetTextColor(dc, priority_color(&card.priority));
        text_out(dc, text_x, tile.top + 5, &format!("{} · {}本", card.priority, card.tavern_tier.unwrap_or(0)));
        SetTextColor(dc, rgb(239, 242, 246));
        for (line_i, line) in wrap_chars(&card.name, 8).into_iter().take(2).enumerate() {
            text_out(dc, text_x, tile.top + 25 + line_i as i32 * 17, &line);
        }
        SetTextColor(dc, rgb(145, 158, 173));
        text_out(dc, text_x, tile.bottom - 21, &truncate_chars(&card.role, 9));
    }

    if let Some((card, tile)) = hovered_card {
        draw_guide_hover_card(dc, width, height, &card, tile);
    }

    ReleaseDC(hwnd, dc);
    hits
}

#[cfg(windows)]
unsafe fn draw_guide_hover_card(
    dc: windows_sys::Win32::Graphics::Gdi::HDC,
    width: i32,
    height: i32,
    card: &super::model::WatchCard,
    tile: UiRect,
) {
    use windows_sys::Win32::Graphics::Gdi::SetTextColor;
    let tooltip_w = ((width as f32 * 0.32) as i32).clamp(270, 390).min(width - 16);
    let tooltip_h = 78.min((height - 35).max(48));
    let center = (tile.left + tile.right) / 2;
    let left = (center - tooltip_w / 2).clamp(8, (width - tooltip_w - 8).max(8));
    let top = (height - tooltip_h - 7).max(31);
    let rect = UiRect {
        left,
        top,
        right: left + tooltip_w,
        bottom: top + tooltip_h,
    };
    fill_ui_rect(dc, rect, rgb(17, 20, 26));
    draw_hollow_frame(dc, rect, 2, priority_color(&card.priority));
    SetTextColor(dc, priority_color(&card.priority));
    text_out(
        dc,
        rect.left + 8,
        rect.top + 6,
        &truncate_chars(
            &format!(
                "{} · {}本 · {}",
                card.priority,
                card.tavern_tier.unwrap_or(0),
                card.name
            ),
            30,
        ),
    );
    SetTextColor(dc, rgb(232, 237, 243));
    text_out(dc, rect.left + 8, rect.top + 27, &truncate_chars(&format!("作用：{}", card.role), 34));
    SetTextColor(dc, rgb(166, 179, 193));
    let cols = ((tooltip_w - 16) / 9).clamp(18, 42) as usize;
    for (index, line) in wrap_chars(&card.reason, cols).into_iter().take(2).enumerate() {
        text_out(dc, rect.left + 8, rect.top + 47 + index as i32 * 16, &line);
    }
}

#[cfg(windows)]
unsafe fn draw_guide_page(
    dc: windows_sys::Win32::Graphics::Gdi::HDC,
    width: i32,
    height: i32,
    public: &super::server::PublicState,
    ui: &mut PanelUiState,
    mut y: i32,
    hits: &mut Vec<HitRegion>,
) {
    use windows_sys::Win32::Graphics::Gdi::SetTextColor;
    let pad = 12;

    if !public.api_key_configured {
        SetTextColor(dc, rgb(255, 191, 94));
        text_out(dc, pad, y, "DeepSeek API Key 未配置");
        y += 24;
        SetTextColor(dc, rgb(200, 207, 216));
        for line in wrap_chars(
            "请编辑 hearthcoach_demo.json 的 deepseek.api_key；本地动态决策仍可工作。",
            29,
        )
        .into_iter()
        .take(3)
        {
            text_out(dc, pad, y, &line);
            y += 20;
        }
        return;
    }

    if let Some(error) = public.error.as_deref() {
        SetTextColor(dc, rgb(255, 112, 112));
        for line in wrap_chars(error, 30).into_iter().take(2) {
            text_out(dc, pad, y, &line);
            y += 19;
        }
        y += 4;
    }

    let Some(selected) = public.selected_composition.as_ref() else {
        draw_composition_picker(dc, width, height, public, y, hits);
        return;
    };

    SetTextColor(dc, rgb(245, 247, 250));
    text_out(
        dc,
        pad,
        y,
        &format!("当前阵容：{}", truncate_chars(&selected.name, 16)),
    );
    let change_rect = UiRect {
        left: width - 92,
        top: y - 4,
        right: width - pad,
        bottom: y + 20,
    };
    draw_button(dc, change_rect, "换阵容", rgb(62, 72, 84));
    hits.push(HitRegion {
        rect: change_rect,
        action: PanelAction::ResetComposition,
    });
    y += 29;

    let Some(watchlist) = public.watchlist.as_ref() else {
        SetTextColor(dc, rgb(190, 199, 209));
        for line in wrap_chars(&public.ai_status, 30).into_iter().take(4) {
            text_out(dc, pad, y, &line);
            y += 20;
        }
        return;
    };

    let stage = ui
        .stage_override
        .as_deref()
        .unwrap_or(public.current_stage.as_str())
        .to_owned();
    let stage_gap = 6;
    let stage_width = (width - pad * 2 - stage_gap * 2) / 3;
    for (idx, (stage_id, label)) in [("early", "前期"), ("mid", "中期"), ("late", "后期")]
        .into_iter()
        .enumerate()
    {
        let left = pad + idx as i32 * (stage_width + stage_gap);
        let rect = UiRect {
            left,
            top: y,
            right: left + stage_width,
            bottom: y + 28,
        };
        draw_button(
            dc,
            rect,
            label,
            if stage == stage_id {
                rgb(50, 111, 164)
            } else {
                rgb(43, 50, 59)
            },
        );
        hits.push(HitRegion {
            rect,
            action: PanelAction::SetStage(stage_id.to_owned()),
        });
    }
    y += 36;

    let mut cards = watchlist
        .stages
        .iter()
        .find(|item| item.stage == stage)
        .map(|item| item.cards.clone())
        .unwrap_or_default();
    cards.sort_by_key(|card| {
        (
            priority_rank(&card.priority),
            card.tavern_tier.unwrap_or(9),
            card.name.clone(),
        )
    });

    if cards.is_empty() {
        SetTextColor(dc, rgb(170, 181, 194));
        text_out(dc, pad, y, "这个阶段没有推荐卡牌。");
        return;
    }

    let max_scroll = cards.len().saturating_sub(1);
    ui.scroll = ui.scroll.min(max_scroll);
    let bottom_reserved = 34;
    let mut index = ui.scroll;
    while index < cards.len() && y < height - bottom_reserved - 40 {
        let card = &cards[index];
        let expanded = ui.expanded_card_id.as_deref() == Some(card.card_id.as_str());
        let row_height = if expanded { 108 } else { 44 };
        if y + row_height > height - bottom_reserved {
            break;
        }

        let row_rect = UiRect {
            left: pad,
            top: y,
            right: width - pad,
            bottom: y + row_height,
        };
        fill_ui_rect(
            dc,
            row_rect,
            if expanded {
                rgb(39, 48, 58)
            } else {
                rgb(29, 36, 44)
            },
        );
        draw_hollow_frame(dc, row_rect, 1, rgb(58, 68, 80));

        let color = priority_color(&card.priority);
        SetTextColor(dc, color);
        text_out(dc, row_rect.left + 7, y + 6, &card.priority);
        SetTextColor(dc, rgb(239, 242, 246));
        let tier = card
            .tavern_tier
            .map(|tier| format!("{tier}本"))
            .unwrap_or_else(|| "?本".to_owned());
        let hit_now = public
            .decision_hits
            .iter()
            .chain(public.shop_hits.iter())
            .any(|hit| hit.card_id == card.card_id);
        let marker = if hit_now { "● " } else { "" };
        text_out(
            dc,
            row_rect.left + 31,
            y + 6,
            &truncate_chars(&format!("{marker}{}  {tier}", card.name), 22),
        );
        SetTextColor(dc, rgb(157, 171, 185));
        text_out(dc, row_rect.left + 31, y + 24, &truncate_chars(&card.role, 22));
        SetTextColor(dc, rgb(130, 145, 160));
        text_out(dc, row_rect.right - 22, y + 13, if expanded { "▲" } else { "▼" });

        if expanded {
            let mut reason_y = y + 45;
            SetTextColor(dc, rgb(207, 215, 224));
            for line in wrap_chars(&card.reason, 30).into_iter().take(3) {
                text_out(dc, row_rect.left + 9, reason_y, &line);
                reason_y += 18;
            }
        }

        hits.push(HitRegion {
            rect: row_rect,
            action: PanelAction::ToggleCard(card.card_id.clone()),
        });
        y += row_height + 5;
        index += 1;
    }

    if ui.scroll > 0 {
        let rect = UiRect {
            left: pad,
            top: height - 30,
            right: width / 2 - 3,
            bottom: height - 8,
        };
        draw_button(dc, rect, "↑ 上一页", rgb(43, 50, 59));
        hits.push(HitRegion {
            rect,
            action: PanelAction::ScrollUp,
        });
    }
    if index < cards.len() {
        let rect = UiRect {
            left: width / 2 + 3,
            top: height - 30,
            right: width - pad,
            bottom: height - 8,
        };
        draw_button(dc, rect, "下一页 ↓", rgb(43, 50, 59));
        hits.push(HitRegion {
            rect,
            action: PanelAction::ScrollDown,
        });
    }
}

#[cfg(windows)]
unsafe fn draw_composition_picker(
    dc: windows_sys::Win32::Graphics::Gdi::HDC,
    width: i32,
    height: i32,
    public: &super::server::PublicState,
    mut y: i32,
    hits: &mut Vec<HitRegion>,
) {
    use windows_sys::Win32::Graphics::Gdi::SetTextColor;
    let pad = 12;

    if public.compositions.is_empty() {
        SetTextColor(dc, rgb(232, 236, 241));
        text_out(dc, pad, y, "先让 AI 根据本局种族推荐阵容");
        y += 30;
        SetTextColor(dc, rgb(165, 177, 190));
        for line in wrap_chars(&public.ai_status, 30).into_iter().take(3) {
            text_out(dc, pad, y, &line);
            y += 19;
        }
        y += 8;
        if !public.ai_status.starts_with("AI 正在") && !public.available_tribes.is_empty() {
            let rect = UiRect { left: pad, top: y, right: width - pad, bottom: y + 36 };
            draw_button(dc, rect, "AI 分析可玩阵容", rgb(41, 119, 184));
            hits.push(HitRegion { rect, action: PanelAction::Analyze });
        }
        return;
    }

    SetTextColor(dc, rgb(239, 242, 246));
    text_out(dc, pad, y, "选择这一局想玩的阵容：");
    y += 26;

    for (idx, comp) in public.compositions.iter().take(5).enumerate() {
        if y + 58 > height - 10 {
            break;
        }
        let rect = UiRect { left: pad, top: y, right: width - pad, bottom: y + 54 };
        fill_ui_rect(dc, rect, rgb(29, 36, 44));
        draw_hollow_frame(dc, rect, 1, rgb(63, 75, 88));
        SetTextColor(dc, rgb(124, 203, 255));
        text_out(dc, rect.left + 8, y + 6, &format!("{}. {}", idx + 1, truncate_chars(&comp.name, 18)));
        SetTextColor(dc, rgb(157, 171, 185));
        text_out(dc, rect.left + 8, y + 26, &truncate_chars(&format!("{} · {}", comp.difficulty, comp.summary), 30));
        hits.push(HitRegion { rect, action: PanelAction::SelectComposition(comp.id.clone()) });
        y += 60;
    }
}

#[cfg(windows)]
unsafe fn draw_chat_window(
    hwnd: windows_sys::Win32::Foundation::HWND,
    width: i32,
    height: i32,
    public: &super::server::PublicState,
    send_rect: UiRect,
    close_rect: UiRect,
) {
    use windows_sys::Win32::{
        Foundation::RECT,
        Graphics::Gdi::{GetDC, ReleaseDC, SetBkMode, SetTextColor, TRANSPARENT},
    };
    let dc = GetDC(hwnd);
    if dc.is_null() {
        return;
    }
    // WS_CLIPCHILDREN prevents this paint from erasing the native history/input
    // EDIT controls. The history itself is rendered by Windows and is scrollable.
    fill(dc, RECT { left: 0, top: 0, right: width, bottom: height }, rgb(18, 21, 27));
    SetBkMode(dc, TRANSPARENT as i32);
    let pad = 12;
    SetTextColor(dc, rgb(245, 247, 250));
    text_out(dc, pad, 8, "和 HearthCoach 交流");
    SetTextColor(dc, rgb(142, 158, 175));
    text_out(dc, pad + 150, 8, "实时状态/HDT事实约束 · 可滚动完整回答");
    draw_button(dc, close_rect, "×", rgb(69, 47, 52));
    draw_separator(dc, pad, width - pad, 34);

    let input_top = height - 92;
    SetTextColor(dc, if public.chat_busy { rgb(214, 180, 99) } else { rgb(132, 149, 166) });
    text_out(
        dc,
        pad,
        (input_top - 20).max(35),
        &truncate_text_pixels(dc, &public.chat_status, (width - 118).max(120)),
    );
    draw_button(
        dc,
        send_rect,
        if public.chat_busy { "思考中" } else { "发送" },
        if public.chat_busy { rgb(72, 72, 72) } else { rgb(45, 111, 164) },
    );
    ReleaseDC(hwnd, dc);
}

#[cfg(windows)]
fn chat_transcript(messages: &[super::model::ChatMessage]) -> String {
    let mut out = String::new();
    for message in messages {
        if !out.is_empty() {
            out.push_str("\r\n\r\n");
        }
        if message.role == "user" {
            out.push_str("你：\r\n");
        } else {
            out.push_str("AI");
            if message.plan_changed {
                out.push_str(" [已修改目标]");
            }
            out.push_str("：\r\n");
        }
        out.push_str(&message.content.replace('\n', "\r\n"));
    }
    out
}

#[cfg(windows)]
fn handle_panel_action(state: &DemoState, ui: &mut PanelUiState, action: PanelAction) {
    match action {
        PanelAction::SetPanelPage(page) => {
            ui.page = page;
            ui.expanded_card_id = None;
            ui.scroll = 0;
        }
        PanelAction::Analyze => {
            if let Err(error) = state.request_analyze() {
                eprintln!("[overlay] analyze: {error}");
            }
        }
        PanelAction::SelectComposition(id) => {
            if let Err(error) = state.request_select(&id) {
                eprintln!("[overlay] select composition: {error}");
            }
        }
        PanelAction::ResetComposition => {
            state.reset_composition_choice();
            ui.stage_override = None;
            ui.expanded_card_id = None;
            ui.scroll = 0;
        }
        PanelAction::SetStage(stage) => {
            ui.stage_override = Some(stage);
            ui.expanded_card_id = None;
            ui.scroll = 0;
        }
        PanelAction::ToggleCard(card_id) => {
            if ui.expanded_card_id.as_deref() == Some(card_id.as_str()) {
                ui.expanded_card_id = None;
            } else {
                ui.expanded_card_id = Some(card_id);
            }
        }
        PanelAction::ScrollUp => {
            ui.scroll = ui.scroll.saturating_sub(4);
            ui.expanded_card_id = None;
        }
        PanelAction::ScrollDown => {
            ui.scroll = ui.scroll.saturating_add(4);
            ui.expanded_card_id = None;
        }
        PanelAction::ToggleTestFrames => {
            state.toggle_overlay_test_mode();
        }
        PanelAction::ResetPanelLayout => {
            if let Err(error) = state.reset_overlay_panel_layout() {
                eprintln!("[overlay] reset panel layout: {error}");
            }
        }
        PanelAction::OpenChat => {
            // Window activation/focus is handled in the overlay loop where the
            // native chat/edit HWNDs are available.
        }
    }
}

#[cfg(windows)]
unsafe fn hearthstone_client_rect(
    hwnd: windows_sys::Win32::Foundation::HWND,
) -> Option<windows_sys::Win32::Foundation::RECT> {
    use windows_sys::Win32::{
        Foundation::{POINT, RECT},
        Graphics::Gdi::ClientToScreen,
        UI::WindowsAndMessaging::{GetClientRect, GetWindowRect},
    };

    let mut client = zero_rect();
    let mut origin = POINT { x: 0, y: 0 };
    if GetClientRect(hwnd, &mut client) != 0 && ClientToScreen(hwnd, &mut origin) != 0 {
        let width = (client.right - client.left).max(1);
        let height = (client.bottom - client.top).max(1);
        return Some(RECT {
            left: origin.x,
            top: origin.y,
            right: origin.x + width,
            bottom: origin.y + height,
        });
    }

    let mut fallback = zero_rect();
    if GetWindowRect(hwnd, &mut fallback) != 0 {
        Some(fallback)
    } else {
        None
    }
}

#[cfg(windows)]
unsafe fn pump_window_messages() {
    use std::ptr::null_mut;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE,
    };
    let mut msg: MSG = std::mem::zeroed();
    while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
        TranslateMessage(&msg);
        DispatchMessageW(&msg);
    }
}

#[cfg(windows)]
unsafe fn find_hearthstone_window() -> windows_sys::Win32::Foundation::HWND {
    use std::ptr::null;
    use windows_sys::Win32::UI::WindowsAndMessaging::FindWindowW;

    let english = wide("Hearthstone");
    let chinese = wide("炉石传说");
    let hwnd = FindWindowW(null(), english.as_ptr());
    if !hwnd.is_null() {
        hwnd
    } else {
        FindWindowW(null(), chinese.as_ptr())
    }
}

#[cfg(windows)]
unsafe fn draw_hollow_frame(
    dc: windows_sys::Win32::Graphics::Gdi::HDC,
    rect: UiRect,
    thickness: i32,
    color: u32,
) {
    let t = thickness.max(1);
    fill_ui_rect(dc, UiRect { left: rect.left, top: rect.top, right: rect.right, bottom: rect.top + t }, color);
    fill_ui_rect(dc, UiRect { left: rect.left, top: rect.bottom - t, right: rect.right, bottom: rect.bottom }, color);
    fill_ui_rect(dc, UiRect { left: rect.left, top: rect.top + t, right: rect.left + t, bottom: rect.bottom - t }, color);
    fill_ui_rect(dc, UiRect { left: rect.right - t, top: rect.top + t, right: rect.right, bottom: rect.bottom - t }, color);
}

#[cfg(windows)]
unsafe fn draw_button(
    dc: windows_sys::Win32::Graphics::Gdi::HDC,
    rect: UiRect,
    label: &str,
    color: u32,
) {
    use windows_sys::Win32::Graphics::Gdi::SetTextColor;
    fill_ui_rect(dc, rect, color);
    SetTextColor(dc, rgb(244, 247, 250));
    let chars = label.chars().count() as i32;
    let x = (rect.left + (rect.right - rect.left - chars * 8) / 2).max(rect.left + 4);
    let y = rect.top + ((rect.bottom - rect.top - 16) / 2).max(1);
    text_out(dc, x, y, label);
}

#[cfg(windows)]
unsafe fn draw_separator(
    dc: windows_sys::Win32::Graphics::Gdi::HDC,
    left: i32,
    right: i32,
    y: i32,
) {
    fill_ui_rect(dc, UiRect { left, top: y, right, bottom: y + 1 }, rgb(57, 66, 77));
}

#[cfg(windows)]
unsafe fn fill_ui_rect(
    dc: windows_sys::Win32::Graphics::Gdi::HDC,
    rect: UiRect,
    color: u32,
) {
    let native = windows_sys::Win32::Foundation::RECT {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    };
    fill(dc, native, color);
}

#[cfg(windows)]
unsafe fn fill(
    dc: windows_sys::Win32::Graphics::Gdi::HDC,
    rect: windows_sys::Win32::Foundation::RECT,
    color: u32,
) {
    use windows_sys::Win32::Graphics::Gdi::{CreateSolidBrush, DeleteObject, FillRect};
    let brush = CreateSolidBrush(color);
    if !brush.is_null() {
        FillRect(dc, &rect, brush);
        DeleteObject(brush);
    }
}

#[cfg(windows)]
unsafe fn text_out(
    dc: windows_sys::Win32::Graphics::Gdi::HDC,
    x: i32,
    y: i32,
    text: &str,
) {
    use windows_sys::Win32::Graphics::Gdi::TextOutW;
    let value = wide(text);
    TextOutW(dc, x, y, value.as_ptr(), (value.len().saturating_sub(1)) as i32);
}

#[cfg(windows)]
unsafe fn text_pixel_width(
    dc: windows_sys::Win32::Graphics::Gdi::HDC,
    text: &str,
) -> i32 {
    use windows_sys::Win32::{
        Foundation::SIZE,
        Graphics::Gdi::GetTextExtentPoint32W,
    };
    if text.is_empty() {
        return 0;
    }
    let value = wide(text);
    let mut size = SIZE { cx: 0, cy: 0 };
    if GetTextExtentPoint32W(
        dc,
        value.as_ptr(),
        value.len().saturating_sub(1) as i32,
        &mut size,
    ) != 0
    {
        size.cx
    } else {
        text.chars().count() as i32 * 9
    }
}

#[cfg(windows)]
unsafe fn wrap_text_pixels(
    dc: windows_sys::Win32::Graphics::Gdi::HDC,
    text: &str,
    max_width: i32,
) -> Vec<String> {
    let max_width = max_width.max(24);
    let mut lines = Vec::new();
    let mut current = String::new();
    for ch in text.chars() {
        if ch == '\n' {
            lines.push(std::mem::take(&mut current));
            continue;
        }
        let mut candidate = current.clone();
        candidate.push(ch);
        if !current.is_empty() && text_pixel_width(dc, &candidate) > max_width {
            lines.push(std::mem::take(&mut current));
        }
        current.push(ch);
    }
    if !current.is_empty() || lines.is_empty() {
        lines.push(current);
    }
    lines
}

#[cfg(windows)]
unsafe fn truncate_text_pixels(
    dc: windows_sys::Win32::Graphics::Gdi::HDC,
    text: &str,
    max_width: i32,
) -> String {
    if text_pixel_width(dc, text) <= max_width {
        return text.to_owned();
    }
    let mut out = String::new();
    for ch in text.chars() {
        let candidate = format!("{out}{ch}…");
        if text_pixel_width(dc, &candidate) > max_width {
            break;
        }
        out.push(ch);
    }
    out.push('…');
    out
}

#[cfg(windows)]
unsafe fn draw_wrapped_text(
    dc: windows_sys::Win32::Graphics::Gdi::HDC,
    x: i32,
    y: i32,
    text: &str,
    max_width: i32,
    max_lines: usize,
    line_height: i32,
) -> i32 {
    let lines = wrap_text_pixels(dc, text, max_width);
    let truncated = lines.len() > max_lines;
    let mut drawn = 0usize;
    for mut line in lines.into_iter().take(max_lines) {
        if truncated && drawn + 1 == max_lines {
            line = truncate_text_pixels(dc, &format!("{line}…"), max_width);
        }
        text_out(dc, x, y + drawn as i32 * line_height, &line);
        drawn += 1;
    }
    drawn as i32 * line_height
}

#[cfg(windows)]
fn priority_rank(priority: &str) -> u8 {
    match priority.trim().to_ascii_uppercase().as_str() {
        "S" => 0,
        "A" => 1,
        _ => 2,
    }
}

#[cfg(windows)]
fn priority_color(priority: &str) -> u32 {
    match priority.trim().to_ascii_uppercase().as_str() {
        "S" => rgb(255, 74, 74),
        "A" => rgb(255, 190, 46),
        _ => rgb(80, 207, 255),
    }
}

#[cfg(windows)]
fn upgrade_posture_cn(posture: super::model::UpgradePosture) -> &'static str {
    match posture {
        super::model::UpgradePosture::Prefer => "优先",
        super::model::UpgradePosture::Neutral => "中性",
        super::model::UpgradePosture::Delay => "延后",
    }
}

#[cfg(windows)]
fn choice_kind_cn(kind: &crate::harness::ChoiceKind) -> &'static str {
    match kind {
        crate::harness::ChoiceKind::Hero => "英雄选择",
        crate::harness::ChoiceKind::Trinket => "饰品选择",
        crate::harness::ChoiceKind::DarkGift => "黑暗赠礼",
        crate::harness::ChoiceKind::Discover => "发现/选择",
        crate::harness::ChoiceKind::Other => "特殊选择",
    }
}

#[cfg(windows)]
fn stage_cn(stage: &str) -> &'static str {
    match stage {
        "early" => "前期",
        "mid" => "中期",
        "late" => "后期",
        _ => "未知阶段",
    }
}

#[cfg(windows)]
fn phase_cn(phase: &str) -> &'static str {
    if phase.eq_ignore_ascii_case("Recruit") {
        "购买阶段"
    } else if phase.eq_ignore_ascii_case("Combat") {
        "战斗阶段"
    } else if phase.eq_ignore_ascii_case("Complete") {
        "已结束"
    } else {
        "准备阶段"
    }
}

#[cfg(windows)]
fn tribe_cn(tribe: &str) -> String {
    match tribe.trim().to_ascii_uppercase().as_str() {
        "BEAST" => "野兽".to_owned(),
        "DEMON" => "恶魔".to_owned(),
        "DRAGON" => "龙".to_owned(),
        "ELEMENTAL" | "ELEMENTALS" => "元素".to_owned(),
        "MECH" | "MECHANICAL" => "机械".to_owned(),
        "MURLOC" => "鱼人".to_owned(),
        "NAGA" => "纳迦".to_owned(),
        "PIRATE" => "海盗".to_owned(),
        "QUILLBOAR" | "QUILBOAR" => "野猪人".to_owned(),
        "UNDEAD" => "亡灵".to_owned(),
        other => other.to_owned(),
    }
}

#[cfg(windows)]
fn wrap_chars(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut count = 0usize;
    for ch in text.chars() {
        if ch == '\n' || count >= width {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            count = 0;
            if ch == '\n' {
                continue;
            }
        }
        current.push(ch);
        count += 1;
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

#[cfg(windows)]
fn truncate_chars(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        return text.to_owned();
    }
    let mut out = text.chars().take(max.saturating_sub(1)).collect::<String>();
    out.push('…');
    out
}

#[cfg(windows)]
fn zero_rect() -> windows_sys::Win32::Foundation::RECT {
    windows_sys::Win32::Foundation::RECT { left: 0, top: 0, right: 0, bottom: 0 }
}

#[cfg(windows)]
fn same_rect(a: &windows_sys::Win32::Foundation::RECT, b: &windows_sys::Win32::Foundation::RECT) -> bool {
    a.left == b.left && a.top == b.top && a.right == b.right && a.bottom == b.bottom
}

#[cfg(windows)]
fn panel_rect_contains(rect: &windows_sys::Win32::Foundation::RECT, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

#[cfg(windows)]
const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    r as u32 | ((g as u32) << 8) | ((b as u32) << 16)
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
