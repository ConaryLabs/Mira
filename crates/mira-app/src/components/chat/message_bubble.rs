// crates/mira-app/src/components/chat/message_bubble.rs
// Chat message bubble component

use leptos::prelude::*;
use super::{Expandable, Markdown};

#[derive(Clone, Debug)]
pub struct ToolCallInfo {
    pub name: String,
    pub arguments: String,
    pub result: Option<String>,
    pub success: bool,
    pub duration_ms: u64,
}

#[component]
pub fn MessageBubble(
    role: String,
    content: String,
    timestamp: Option<String>,
    thinking: Option<String>,
    tool_calls: Option<Vec<ToolCallInfo>>,
) -> impl IntoView {
    let is_user = role == "user";
    let role_display = if is_user { "You" } else { "Assistant" };
    let avatar = if is_user { "U" } else { "A" };

    let message_class = if is_user { "message user" } else { "message assistant" };

    let tool_count = tool_calls.as_ref().map(|t| t.len()).unwrap_or(0);
    let has_tools = tool_count > 0;
    let _ = thinking; // Unused - reasoning stored in DB but not displayed

    view! {
        <div class=message_class>
            <div class="message-avatar">{avatar}</div>
            <div class="message-bubble">
                <div class="message-header">
                    <span class="message-role">{role_display}</span>
                    {timestamp.map(|t| view! { <span class="message-time">{t}</span> })}
                </div>

                <div class="message-content">
                    <Markdown content=content.clone()/>
                </div>

                // Tool calls section (collapsed by default)
                {has_tools.then(|| {
                    let calls = tool_calls.clone().unwrap_or_default();
                    let badge = format!("{}", tool_count);
                    view! {
                        <Expandable label="Tools" badge=badge>
                            <div class="space-y-2">
                                {calls.into_iter().map(|tc| {
                                    let status_class = if tc.success { "text-success" } else { "text-error" };
                                    view! {
                                        <div class="tool-call">
                                            <div class="flex items-center gap-2 mb-1">
                                                <span class="font-semibold">{tc.name.clone()}</span>
                                                <span class=format!("text-xs {}", status_class)>
                                                    {if tc.success { "OK" } else { "FAIL" }}
                                                </span>
                                                <span class="text-xs text-muted ml-auto">
                                                    {format!("{}ms", tc.duration_ms)}
                                                </span>
                                            </div>
                                            {(!tc.arguments.is_empty()).then(|| {
                                                view! {
                                                    <pre class="text-xs text-muted whitespace-pre-wrap">
                                                        {tc.arguments.clone()}
                                                    </pre>
                                                }
                                            })}
                                            {tc.result.map(|r| {
                                                view! {
                                                    <pre class="text-xs mt-1 whitespace-pre-wrap">
                                                        {r}
                                                    </pre>
                                                }
                                            })}
                                        </div>
                                    }
                                }).collect::<Vec<_>>()}
                            </div>
                        </Expandable>
                    }
                })}
            </div>
        </div>
    }
}
