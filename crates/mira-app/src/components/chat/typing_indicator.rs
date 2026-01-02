// crates/mira-app/src/components/chat/typing_indicator.rs
// Typing and thinking indicators

use leptos::prelude::*;

#[component]
pub fn TypingIndicator() -> impl IntoView {
    view! {
        <div class="typing-indicator">
            <div class="typing-dot"></div>
            <div class="typing-dot"></div>
            <div class="typing-dot"></div>
        </div>
    }
}

#[component]
pub fn ThinkingIndicator(
    #[prop(optional)] label: Option<&'static str>,
) -> impl IntoView {
    view! {
        <div class="thinking-indicator">
            <div class="thinking-ring"></div>
            <span>{label.unwrap_or("Thinking...")}</span>
        </div>
    }
}
