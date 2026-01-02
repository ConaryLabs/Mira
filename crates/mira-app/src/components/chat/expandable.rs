// crates/mira-app/src/components/chat/expandable.rs
// Generic expandable/collapsible section

use leptos::prelude::*;

#[component]
pub fn Expandable(
    label: &'static str,
    #[prop(optional)] badge: Option<String>,
    #[prop(default = false)] default_open: bool,
    children: Children,
) -> impl IntoView {
    let (open, set_open) = signal(default_open);

    view! {
        <div class=move || if open.get() { "expandable open" } else { "expandable" }>
            <div class="expandable-header" on:click=move |_| set_open.update(|o| *o = !*o)>
                <span class="expandable-icon">
                    <svg width="12" height="12" viewBox="0 0 12 12" fill="currentColor">
                        <path d="M4.5 2L9 6L4.5 10" stroke="currentColor" stroke-width="1.5" fill="none"/>
                    </svg>
                </span>
                <span class="expandable-label">{label}</span>
                {badge.map(|b| view! { <span class="expandable-badge">{b}</span> })}
            </div>
            <div class="expandable-content">
                <div class="expandable-inner">
                    {children()}
                </div>
            </div>
        </div>
    }
}
