// crates/mira-app/src/pages/chat.rs
// Chat page - DeepSeek Reasoner integration

use leptos::prelude::*;
use leptos::html;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen_futures::spawn_local;
use web_sys::WebSocket;
use mira_types::WsEvent;
use crate::api::send_chat_message;
use crate::websocket::connect_websocket;
use crate::Layout;

/// Chat message for display
#[derive(Clone, Debug)]
pub struct ChatDisplayMessage {
    pub role: String, // "user" | "assistant" | "thinking" | "tool"
    pub content: String,
    pub timestamp: String,
}

fn chrono_now() -> String {
    js_sys::Date::new_0().to_iso_string().as_string().unwrap_or_default()
}

#[component]
pub fn ChatPage() -> impl IntoView {
    let (messages, set_messages) = signal(Vec::<ChatDisplayMessage>::new());
    let (input, set_input) = signal(String::new());
    let (loading, set_loading) = signal(false);
    let (thinking, set_thinking) = signal(String::new());

    // WebSocket for streaming events
    let (events, set_events) = signal(Vec::<WsEvent>::new());
    let (ws_connected, set_ws_connected) = signal(false);
    let ws_ref: Rc<RefCell<Option<WebSocket>>> = Rc::new(RefCell::new(None));

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
                    set_thinking.update(|t| t.push_str(content));
                }
                WsEvent::ChatChunk { content, .. } => {
                    set_messages.update(|msgs| {
                        if let Some(last) = msgs.last_mut() {
                            if last.role == "assistant" {
                                last.content.push_str(content);
                                return;
                            }
                        }
                        msgs.push(ChatDisplayMessage {
                            role: "assistant".to_string(),
                            content: content.clone(),
                            timestamp: chrono_now(),
                        });
                    });
                }
                WsEvent::ChatComplete { content, .. } => {
                    set_loading.set(false);
                    // Add thinking block if we have one
                    let think_content = thinking.get();
                    if !think_content.is_empty() {
                        set_messages.update(|msgs| {
                            // Insert thinking before the last assistant message
                            let len = msgs.len();
                            if len > 0 {
                                msgs.insert(len - 1, ChatDisplayMessage {
                                    role: "thinking".to_string(),
                                    content: think_content.clone(),
                                    timestamp: chrono_now(),
                                });
                            }
                        });
                        set_thinking.set(String::new());
                    }
                    // Update final assistant message
                    if !content.is_empty() {
                        set_messages.update(|msgs| {
                            if let Some(last) = msgs.last_mut() {
                                if last.role == "assistant" {
                                    last.content = content.clone();
                                }
                            }
                        });
                    }
                }
                WsEvent::ToolStart { tool_name, .. } => {
                    set_messages.update(|msgs| {
                        msgs.push(ChatDisplayMessage {
                            role: "tool".to_string(),
                            content: format!("Calling {}...", tool_name),
                            timestamp: chrono_now(),
                        });
                    });
                }
                WsEvent::ToolResult { tool_name, success, .. } => {
                    set_messages.update(|msgs| {
                        if let Some(last) = msgs.last_mut() {
                            if last.role == "tool" {
                                last.content = format!(
                                    "{}: {}",
                                    tool_name,
                                    if *success { "ok" } else { "error" }
                                );
                            }
                        }
                    });
                }
                WsEvent::ChatError { message } => {
                    set_loading.set(false);
                    set_messages.update(|msgs| {
                        msgs.push(ChatDisplayMessage {
                            role: "error".to_string(),
                            content: message.clone(),
                            timestamp: chrono_now(),
                        });
                    });
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
        set_messages.update(|msgs| {
            msgs.push(ChatDisplayMessage {
                role: "user".to_string(),
                content: msg.clone(),
                timestamp: chrono_now(),
            });
        });

        // Clear input and set loading
        set_input.set(String::new());
        set_loading.set(true);
        set_thinking.set(String::new());

        // Send to API
        spawn_local(async move {
            if let Err(e) = send_chat_message(&msg).await {
                set_loading.set(false);
                set_messages.update(|msgs| {
                    msgs.push(ChatDisplayMessage {
                        role: "error".to_string(),
                        content: e,
                        timestamp: chrono_now(),
                    });
                });
            }
        });
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
        <Layout>
            <div class="max-w-4xl mx-auto h-[calc(100vh-8rem)] flex flex-col">
                <div class="flex items-center justify-between mb-4">
                    <h1 class="text-2xl font-bold">"Chat"</h1>
                    <div class="flex items-center gap-2">
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

                // Messages area
                <div
                    node_ref=messages_ref
                    class="flex-1 overflow-y-auto space-y-4 p-4 bg-card rounded-lg border border-border"
                >
                    {move || {
                        let msgs = messages.get();
                        if msgs.is_empty() {
                            view! {
                                <div class="text-center text-muted py-8">
                                    <p class="mb-2">"Start a conversation"</p>
                                    <p class="text-xs">"I can search memories, code, manage tasks, and spawn Claude Code for file operations."</p>
                                </div>
                            }.into_any()
                        } else {
                            view! {
                                <ChatMessages messages=msgs/>
                            }.into_any()
                        }
                    }}

                    // Loading indicator
                    {move || loading.get().then(|| {
                        let think = thinking.get();
                        let think_display = if think.is_empty() { "...".to_string() } else { think.clone() };
                        view! {
                            <div class="p-3 rounded-lg bg-purple-500/10">
                                <div class="text-xs text-muted mb-1">"Thinking..."</div>
                                <div class="text-sm text-purple-300 italic whitespace-pre-wrap">
                                    {think_display}
                                </div>
                            </div>
                        }
                    })}
                </div>

                // Input area
                <div class="mt-4 flex gap-2">
                    <input
                        type="text"
                        placeholder="Ask anything..."
                        class="flex-1 p-3 bg-card border border-border rounded-lg focus:border-accent outline-none"
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
                        class="px-6 py-3 bg-accent text-background rounded-lg hover:opacity-90 disabled:opacity-50"
                        prop:disabled=move || loading.get() || input.get().is_empty()
                        on:click=move |_| send_message(())
                    >
                        {move || if loading.get() { "..." } else { "Send" }}
                    </button>
                </div>
            </div>
        </Layout>
    }
}

#[component]
fn ChatMessages(messages: Vec<ChatDisplayMessage>) -> impl IntoView {
    view! {
        <For
            each={move || messages.clone().into_iter().enumerate().collect::<Vec<_>>()}
            key={|(idx, msg)| format!("{}-{}", idx, msg.timestamp)}
            children={move |(_, msg)| {
                view! { <ChatMessageBubble msg=msg/> }
            }}
        />
    }
}

#[component]
fn ChatMessageBubble(msg: ChatDisplayMessage) -> impl IntoView {
    let role_class = match msg.role.as_str() {
        "user" => "bg-accent/10 ml-12",
        "assistant" => "bg-background mr-12",
        "thinking" => "bg-purple-500/10 text-purple-300 text-sm italic",
        "tool" => "bg-blue-500/10 text-blue-300 text-sm font-mono",
        "error" => "bg-error/10 text-error",
        _ => "bg-background",
    };
    let label = match msg.role.as_str() {
        "user" => "You",
        "assistant" => "DeepSeek",
        "thinking" => "Thinking",
        "tool" => "Tool",
        "error" => "Error",
        _ => "",
    };
    view! {
        <div class=format!("p-3 rounded-lg {}", role_class)>
            <div class="text-xs text-muted mb-1">{label}</div>
            <div class="whitespace-pre-wrap">{msg.content.clone()}</div>
        </div>
    }
}
