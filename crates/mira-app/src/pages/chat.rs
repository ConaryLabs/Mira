// crates/mira-app/src/pages/chat.rs
// Chat page - DeepSeek Reasoner integration with enhanced UI

use leptos::prelude::*;
use leptos::html;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen_futures::spawn_local;
use web_sys::WebSocket;
use mira_types::{WsEvent, ProjectContext};
use crate::api::{send_chat_message, fetch_projects, fetch_current_project, set_project};
use crate::websocket::connect_websocket;
use crate::components::chat::{MessageBubble, TypingIndicator, ThinkingIndicator, ToolCallInfo};
use crate::ProjectSidebar;

// ═══════════════════════════════════════
// DATA STRUCTURES
// ═══════════════════════════════════════

#[derive(Clone, Debug)]
pub struct ChatDisplayMessage {
    pub id: usize,
    pub role: String,
    pub content: String,
    pub timestamp: String,
    pub thinking: Option<String>,
    pub tool_calls: Vec<ToolCallInfo>,
}

fn chrono_now() -> String {
    let date = js_sys::Date::new_0();
    let hours = date.get_hours();
    let minutes = date.get_minutes();
    format!("{:02}:{:02}", hours, minutes)
}

// ═══════════════════════════════════════
// CHAT PAGE COMPONENT
// ═══════════════════════════════════════

#[component]
pub fn ChatPage() -> impl IntoView {
    // Message state
    let (messages, set_messages) = signal(Vec::<ChatDisplayMessage>::new());
    let (input, set_input) = signal(String::new());
    let (loading, set_loading) = signal(false);
    let (current_thinking, set_current_thinking) = signal(String::new());
    let (message_id_counter, set_message_id_counter) = signal(0usize);

    // Tool calls accumulator (for current assistant response)
    let (pending_tool_calls, set_pending_tool_calls) = signal(Vec::<ToolCallInfo>::new());
    // Store tool start time as f64 (js timestamp) to avoid Send issues with js_sys::Date
    let (current_tool, set_current_tool) = signal(Option::<(String, f64)>::None);

    // Project state
    let (projects, set_projects) = signal(Vec::<ProjectContext>::new());
    let (current_project, set_current_project) = signal(Option::<ProjectContext>::None);
    let sidebar_open = RwSignal::new(false);

    // WebSocket state
    let (events, set_events) = signal(Vec::<WsEvent>::new());
    let (ws_connected, set_ws_connected) = signal(false);
    let ws_ref: Rc<RefCell<Option<WebSocket>>> = Rc::new(RefCell::new(None));

    // Load projects on mount
    Effect::new(move |_| {
        spawn_local(async move {
            if let Ok(projs) = fetch_projects().await {
                set_projects.set(projs);
            }
            if let Ok(Some(proj)) = fetch_current_project().await {
                set_current_project.set(Some(proj));
            }
        });
    });

    // Connect to WebSocket
    let ws_ref_clone = ws_ref.clone();
    Effect::new(move |_| {
        let ws_ref = ws_ref_clone.clone();
        spawn_local(async move {
            connect_websocket(ws_ref, set_events, set_ws_connected);
        });
    });

    // Process incoming events
    Effect::new(move |_| {
        let evts = events.get();
        for event in evts.iter() {
            match event {
                WsEvent::Thinking { content, .. } => {
                    set_current_thinking.update(|t| t.push_str(content));
                }
                WsEvent::ChatChunk { content, .. } => {
                    set_messages.update(|msgs| {
                        if let Some(last) = msgs.last_mut() {
                            if last.role == "assistant" {
                                last.content.push_str(content);
                                return;
                            }
                        }
                        // New assistant message
                        set_message_id_counter.update(|id| *id += 1);
                        let id = message_id_counter.get();
                        msgs.push(ChatDisplayMessage {
                            id,
                            role: "assistant".to_string(),
                            content: content.clone(),
                            timestamp: chrono_now(),
                            thinking: None,
                            tool_calls: vec![],
                        });
                    });
                }
                WsEvent::ChatComplete { content, .. } => {
                    set_loading.set(false);

                    // Get accumulated thinking and tool calls
                    let think_content = current_thinking.get();
                    let tool_calls = pending_tool_calls.get();

                    // Update the last assistant message with thinking and tool calls
                    set_messages.update(|msgs| {
                        if let Some(last) = msgs.last_mut() {
                            if last.role == "assistant" {
                                if !content.is_empty() {
                                    last.content = content.clone();
                                }
                                if !think_content.is_empty() {
                                    last.thinking = Some(think_content.clone());
                                }
                                if !tool_calls.is_empty() {
                                    last.tool_calls = tool_calls.clone();
                                }
                            }
                        }
                    });

                    // Clear accumulators
                    set_current_thinking.set(String::new());
                    set_pending_tool_calls.set(vec![]);
                    set_current_tool.set(None);
                }
                WsEvent::ToolStart { tool_name, arguments, .. } => {
                    set_current_tool.set(Some((tool_name.clone(), js_sys::Date::now())));

                    // Add pending tool call - convert serde_json::Value to String
                    let args_str = serde_json::to_string_pretty(arguments).unwrap_or_default();
                    set_pending_tool_calls.update(|calls| {
                        calls.push(ToolCallInfo {
                            name: tool_name.clone(),
                            arguments: args_str,
                            result: None,
                            success: true,
                            duration_ms: 0,
                        });
                    });
                }
                WsEvent::ToolResult { tool_name, result, success, .. } => {
                    // Calculate duration
                    let duration_ms = if let Some((name, start_time)) = current_tool.get() {
                        if name == *tool_name {
                            (js_sys::Date::now() - start_time) as u64
                        } else {
                            0
                        }
                    } else {
                        0
                    };

                    // Update the last tool call with result
                    set_pending_tool_calls.update(|calls| {
                        if let Some(last) = calls.last_mut() {
                            if last.name == *tool_name {
                                last.result = Some(result.clone());
                                last.success = *success;
                                last.duration_ms = duration_ms;
                            }
                        }
                    });

                    set_current_tool.set(None);
                }
                WsEvent::ChatError { message } => {
                    set_loading.set(false);
                    set_message_id_counter.update(|id| *id += 1);
                    let id = message_id_counter.get();
                    set_messages.update(|msgs| {
                        msgs.push(ChatDisplayMessage {
                            id,
                            role: "error".to_string(),
                            content: message.clone(),
                            timestamp: chrono_now(),
                            thinking: None,
                            tool_calls: vec![],
                        });
                    });
                    set_current_thinking.set(String::new());
                    set_pending_tool_calls.set(vec![]);
                }
                _ => {}
            }
        }
    });

    // Send message handler
    let send_message = move |_| {
        let msg = input.get();
        if msg.is_empty() || loading.get() {
            return;
        }

        // Add user message
        set_message_id_counter.update(|id| *id += 1);
        let id = message_id_counter.get();
        set_messages.update(|msgs| {
            msgs.push(ChatDisplayMessage {
                id,
                role: "user".to_string(),
                content: msg.clone(),
                timestamp: chrono_now(),
                thinking: None,
                tool_calls: vec![],
            });
        });

        // Clear input and set loading
        set_input.set(String::new());
        set_loading.set(true);
        set_current_thinking.set(String::new());
        set_pending_tool_calls.set(vec![]);

        // Send to API
        spawn_local(async move {
            if let Err(e) = send_chat_message(&msg).await {
                set_loading.set(false);
                set_message_id_counter.update(|id| *id += 1);
                let id = message_id_counter.get();
                set_messages.update(|msgs| {
                    msgs.push(ChatDisplayMessage {
                        id,
                        role: "error".to_string(),
                        content: e,
                        timestamp: chrono_now(),
                        thinking: None,
                        tool_calls: vec![],
                    });
                });
            }
        });
    };

    // Project selection handler
    let on_project_select = move |proj: Option<ProjectContext>| {
        if let Some(p) = proj.clone() {
            spawn_local(async move {
                if let Ok(updated) = set_project(&p.path, p.name.as_deref()).await {
                    set_current_project.set(Some(updated));
                }
            });
        } else {
            set_current_project.set(None);
        }
    };

    // Reference for auto-scroll
    let messages_ref = NodeRef::<html::Div>::new();

    // Auto-scroll on new messages
    Effect::new(move |_| {
        let _ = messages.get();
        if let Some(el) = messages_ref.get() {
            el.set_scroll_top(el.scroll_height());
        }
    });

    view! {
        // Project Sidebar
        <ProjectSidebar
            open=sidebar_open
            projects=Signal::derive(move || projects.get())
            current_project=Signal::derive(move || current_project.get())
            on_select=on_project_select
        />

        <div class="min-h-screen flex flex-col bg-background">
            // Chat Header
            <div class="chat-header">
                <button
                    class="chat-menu-btn"
                    on:click=move |_| sidebar_open.set(true)
                >
                    <svg width="16" height="16" viewBox="0 0 16 16" fill="currentColor">
                        <path d="M2 4h12M2 8h12M2 12h12" stroke="currentColor" stroke-width="1.5" fill="none"/>
                    </svg>
                </button>

                <div class="chat-project-name">
                    {move || {
                        if let Some(proj) = current_project.get() {
                            view! { <span>{proj.name}</span> }.into_any()
                        } else {
                            view! { <span class="text-muted">"No Project"</span> }.into_any()
                        }
                    }}
                </div>

                <div class="ml-auto flex items-center gap-2">
                    <div class=move || {
                        if ws_connected.get() {
                            "w-2 h-2 rounded-full bg-success"
                        } else {
                            "w-2 h-2 rounded-full bg-error"
                        }
                    }></div>
                    <span class="text-xs text-muted">"DeepSeek Reasoner"</span>
                </div>
            </div>

            // Messages Area
            <div class="flex-1 overflow-y-auto" node_ref=messages_ref>
                {move || {
                    let msgs = messages.get();
                    if msgs.is_empty() && !loading.get() {
                        view! {
                            <div class="flex items-center justify-center h-full">
                                <div class="text-center text-muted py-12">
                                    <div class="text-4xl mb-4">">"</div>
                                    <p class="mb-2">"Start a conversation"</p>
                                    <p class="text-xs max-w-md">
                                        "I can search memories, code, manage tasks, and spawn Claude Code for file operations."
                                    </p>
                                </div>
                            </div>
                        }.into_any()
                    } else {
                        view! {
                            <div class="chat-container">
                                <For
                                    each=move || messages.get()
                                    key=|msg| msg.id
                                    children=move |msg| {
                                        view! {
                                            <MessageBubble
                                                role=msg.role.clone()
                                                content=msg.content.clone()
                                                timestamp=Some(msg.timestamp.clone())
                                                thinking=msg.thinking.clone()
                                                tool_calls=if msg.tool_calls.is_empty() { None } else { Some(msg.tool_calls.clone()) }
                                            />
                                        }
                                    }
                                />

                                // Loading state
                                {move || loading.get().then(|| {
                                    let think = current_thinking.get();
                                    let has_tool = current_tool.get().is_some();

                                    view! {
                                        <div class="message assistant">
                                            <div class="message-avatar">"A"</div>
                                            <div class="message-bubble">
                                                <div class="message-header">
                                                    <span class="message-role">"Assistant"</span>
                                                </div>
                                                {if !think.is_empty() {
                                                    view! {
                                                        <ThinkingIndicator label="Reasoning..."/>
                                                    }.into_any()
                                                } else if has_tool {
                                                    let tool_name = current_tool.get().map(|(n, _)| n).unwrap_or_default();
                                                    view! {
                                                        <ThinkingIndicator label=Box::leak(format!("Using {}...", tool_name).into_boxed_str())/>
                                                    }.into_any()
                                                } else {
                                                    view! {
                                                        <TypingIndicator/>
                                                    }.into_any()
                                                }}
                                            </div>
                                        </div>
                                    }
                                })}
                            </div>
                        }.into_any()
                    }
                }}
            </div>

            // Input Area
            <div class="chat-input-area">
                <div class="chat-input-wrapper">
                    <input
                        type="text"
                        placeholder="Ask anything..."
                        class="chat-input"
                        prop:value=move || input.get()
                        prop:disabled=move || loading.get()
                        on:input=move |ev| set_input.set(event_target_value(&ev))
                        on:keydown=move |ev| {
                            if ev.key() == "Enter" && !ev.shift_key() {
                                ev.prevent_default();
                                send_message(());
                            }
                        }
                    />
                    <button
                        class="chat-send-btn"
                        prop:disabled=move || loading.get() || input.get().is_empty()
                        on:click=move |_| send_message(())
                    >
                        "Send"
                    </button>
                </div>
            </div>
        </div>
    }
}
