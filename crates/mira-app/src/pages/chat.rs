// crates/mira-app/src/pages/chat.rs
// Chat page - DeepSeek Reasoner integration with SSE streaming

use leptos::prelude::*;
use leptos::html;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use mira_types::ProjectContext;
use crate::api::{fetch_projects, fetch_current_project, set_project, fetch_chat_history, get_api_url};
use crate::components::chat::{MessageBubble, TypingIndicator, ThinkingIndicator, ToolCallInfo};
use crate::ProjectSidebar;

// =============================================
// DATA STRUCTURES
// =============================================

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

fn extract_time(timestamp: &str) -> String {
    if let Some(time_part) = timestamp.split(' ').nth(1) {
        time_part.chars().take(5).collect()
    } else {
        timestamp.chars().take(5).collect()
    }
}

// =============================================
// SSE EVENT TYPES (must match server)
// =============================================

#[derive(Clone, Debug, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ChatEvent {
    Start,
    Delta { content: String },
    Reasoning { content: String },
    ToolStart { name: String, call_id: String },
    ToolResult { name: String, call_id: String, success: bool },
    Done { content: String },
    Error { message: String },
}

// =============================================
// CHAT PAGE COMPONENT
// =============================================

#[component]
pub fn ChatPage() -> impl IntoView {
    // Message state
    let (messages, set_messages) = signal(Vec::<ChatDisplayMessage>::new());
    let (input, set_input) = signal(String::new());
    let (loading, set_loading) = signal(false);
    let (is_thinking, set_is_thinking) = signal(false);
    let (message_id_counter, set_message_id_counter) = signal(0usize);

    // Tool calls accumulator
    let (pending_tool_calls, set_pending_tool_calls) = signal(Vec::<ToolCallInfo>::new());
    let (current_tool, set_current_tool) = signal(Option::<String>::None);

    // Project state
    let (projects, set_projects) = signal(Vec::<ProjectContext>::new());
    let (current_project, set_current_project) = signal(Option::<ProjectContext>::None);
    let sidebar_open = RwSignal::new(false);

    // Load projects and chat history on mount
    Effect::new(move |_| {
        spawn_local(async move {
            if let Ok(projs) = fetch_projects().await {
                set_projects.set(projs);
            }
            if let Ok(Some(proj)) = fetch_current_project().await {
                set_current_project.set(Some(proj));
            }

            if let Ok(history) = fetch_chat_history().await {
                let mut counter = 0usize;
                let loaded_messages: Vec<ChatDisplayMessage> = history
                    .into_iter()
                    .map(|msg| {
                        counter += 1;
                        ChatDisplayMessage {
                            id: counter,
                            role: msg.role,
                            content: msg.content,
                            timestamp: extract_time(&msg.timestamp),
                            thinking: None,
                            tool_calls: vec![],
                        }
                    })
                    .collect();

                if !loaded_messages.is_empty() {
                    set_message_id_counter.set(counter);
                    set_messages.set(loaded_messages);
                }
            }
        });
    });

    // Send message handler using SSE
    let send_message = move |_| {
        let msg = input.get();
        if msg.is_empty() || loading.get() {
            return;
        }

        // Add user message
        set_message_id_counter.update(|id| *id += 1);
        let user_id = message_id_counter.get();
        set_messages.update(|msgs| {
            msgs.push(ChatDisplayMessage {
                id: user_id,
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
        set_is_thinking.set(false);
        set_pending_tool_calls.set(vec![]);
        set_current_tool.set(None);

        // Create assistant message placeholder
        set_message_id_counter.update(|id| *id += 1);
        let assistant_id = message_id_counter.get();
        set_messages.update(|msgs| {
            msgs.push(ChatDisplayMessage {
                id: assistant_id,
                role: "assistant".to_string(),
                content: String::new(),
                timestamp: chrono_now(),
                thinking: None,
                tool_calls: vec![],
            });
        });

        // Open SSE connection
        let url = get_api_url("/api/chat/stream");

        spawn_local(async move {
            // We need to POST with body, but EventSource only supports GET
            // So we'll use fetch with streaming instead
            let window = web_sys::window().unwrap();
            let request_init = web_sys::RequestInit::new();
            request_init.set_method("POST");

            let headers = web_sys::Headers::new().unwrap();
            headers.set("Content-Type", "application/json").unwrap();
            request_init.set_headers(&headers);

            let body = serde_json::json!({ "message": msg });
            request_init.set_body(&JsValue::from_str(&body.to_string()));

            let request = web_sys::Request::new_with_str_and_init(&url, &request_init).unwrap();

            let resp_promise = window.fetch_with_request(&request);
            let resp = wasm_bindgen_futures::JsFuture::from(resp_promise).await;

            match resp {
                Ok(resp_value) => {
                    let response: web_sys::Response = resp_value.dyn_into().unwrap();

                    if !response.ok() {
                        set_loading.set(false);
                        set_messages.update(|msgs| {
                            if let Some(last) = msgs.last_mut() {
                                if last.role == "assistant" {
                                    last.content = format!("Error: HTTP {}", response.status());
                                }
                            }
                        });
                        return;
                    }

                    // Get the response body as a ReadableStream
                    if let Some(body) = response.body() {
                        let reader: web_sys::ReadableStreamDefaultReader = body.get_reader().unchecked_into();
                        let decoder = web_sys::TextDecoder::new().unwrap();
                        let mut buffer = String::new();

                        loop {
                            let read_promise: js_sys::Promise = reader.read();
                            let result = wasm_bindgen_futures::JsFuture::from(read_promise).await;

                            match result {
                                Ok(chunk) => {
                                    let done = js_sys::Reflect::get(&chunk, &JsValue::from_str("done"))
                                        .unwrap()
                                        .as_bool()
                                        .unwrap_or(true);

                                    if done {
                                        break;
                                    }

                                    let value = js_sys::Reflect::get(&chunk, &JsValue::from_str("value")).unwrap();
                                    if !value.is_undefined() {
                                        let array = js_sys::Uint8Array::new(&value);
                                        let text = decoder.decode_with_buffer_source(&array).unwrap_or_default();
                                        buffer.push_str(&text);

                                        // Process SSE events in buffer
                                        while let Some(pos) = buffer.find("\n\n") {
                                            let event_str = buffer[..pos].to_string();
                                            buffer = buffer[pos + 2..].to_string();

                                            // Parse SSE event
                                            for line in event_str.lines() {
                                                if let Some(data) = line.strip_prefix("data: ") {
                                                    if let Ok(event) = serde_json::from_str::<ChatEvent>(data) {
                                                        match event {
                                                            ChatEvent::Start => {
                                                                set_is_thinking.set(true);
                                                            }
                                                            ChatEvent::Delta { content } => {
                                                                set_is_thinking.set(false);
                                                                set_messages.update(|msgs| {
                                                                    if let Some(last) = msgs.last_mut() {
                                                                        if last.role == "assistant" {
                                                                            last.content.push_str(&content);
                                                                        }
                                                                    }
                                                                });
                                                            }
                                                            ChatEvent::Reasoning { .. } => {
                                                                // Could store for display
                                                            }
                                                            ChatEvent::ToolStart { name, call_id } => {
                                                                set_current_tool.set(Some(name.clone()));
                                                                set_pending_tool_calls.update(|calls| {
                                                                    calls.push(ToolCallInfo {
                                                                        name,
                                                                        arguments: String::new(),
                                                                        result: None,
                                                                        success: true,
                                                                        duration_ms: 0,
                                                                    });
                                                                });
                                                            }
                                                            ChatEvent::ToolResult { name, success, .. } => {
                                                                set_current_tool.set(None);
                                                                set_pending_tool_calls.update(|calls| {
                                                                    if let Some(last) = calls.last_mut() {
                                                                        if last.name == name {
                                                                            last.success = success;
                                                                        }
                                                                    }
                                                                });
                                                            }
                                                            ChatEvent::Done { content } => {
                                                                set_loading.set(false);
                                                                set_is_thinking.set(false);
                                                                let tool_calls = pending_tool_calls.get();
                                                                set_messages.update(|msgs| {
                                                                    if let Some(last) = msgs.last_mut() {
                                                                        if last.role == "assistant" {
                                                                            last.content = content;
                                                                            if !tool_calls.is_empty() {
                                                                                last.tool_calls = tool_calls;
                                                                            }
                                                                        }
                                                                    }
                                                                });
                                                                set_pending_tool_calls.set(vec![]);
                                                            }
                                                            ChatEvent::Error { message } => {
                                                                set_loading.set(false);
                                                                set_is_thinking.set(false);
                                                                set_messages.update(|msgs| {
                                                                    if let Some(last) = msgs.last_mut() {
                                                                        if last.role == "assistant" {
                                                                            last.content = format!("Error: {}", message);
                                                                        }
                                                                    }
                                                                });
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                Err(_) => break,
                            }
                        }
                    }
                }
                Err(e) => {
                    set_loading.set(false);
                    set_messages.update(|msgs| {
                        if let Some(last) = msgs.last_mut() {
                            if last.role == "assistant" {
                                last.content = format!("Connection error: {:?}", e);
                            }
                        }
                    });
                }
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
                                {move || {
                                    let is_loading = loading.get();
                                    let thinking = is_thinking.get();
                                    let has_tool = current_tool.get().is_some();

                                    // Only show loading indicator if we're loading AND the last message has no content yet
                                    let show_loading = is_loading && {
                                        let msgs = messages.get();
                                        msgs.last()
                                            .map(|m| m.role == "assistant" && m.content.is_empty())
                                            .unwrap_or(false)
                                    };

                                    show_loading.then(|| {
                                        view! {
                                            <div class="message assistant">
                                                <div class="message-avatar">"A"</div>
                                                <div class="message-bubble">
                                                    <div class="message-header">
                                                        <span class="message-role">"Assistant"</span>
                                                    </div>
                                                    {if thinking {
                                                        view! {
                                                            <ThinkingIndicator label="Reasoning..."/>
                                                        }.into_any()
                                                    } else if has_tool {
                                                        let tool_name = current_tool.get().unwrap_or_default();
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
                                    })
                                }}
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
