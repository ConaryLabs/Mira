// crates/mira-app/src/components.rs
// Shared layout components

use leptos::prelude::*;

use crate::ConnectionState;

// ============================================================================
// Layout Components
// ============================================================================

#[component]
pub fn Layout(children: Children) -> impl IntoView {
    view! {
        <div class="min-h-screen flex flex-col">
            <Nav/>
            <main class="flex-1 p-4">
                {children()}
            </main>
        </div>
    }
}

#[component]
pub fn Nav() -> impl IntoView {
    view! {
        <nav class="border-b border-border px-4 py-3 flex items-center gap-6">
            <a href="/" class="text-accent font-bold text-lg">"Mira Studio"</a>
            <div class="flex gap-4 text-sm">
                <a href="/chat" class="hover:text-accent transition-colors">"Chat"</a>
                <a href="/ghost" class="hover:text-accent transition-colors">"Ghost Mode"</a>
                <a href="/memories" class="hover:text-accent transition-colors">"Memories"</a>
                <a href="/code" class="hover:text-accent transition-colors">"Code"</a>
                <a href="/tasks" class="hover:text-accent transition-colors">"Tasks"</a>
            </div>
            <div class="flex-1"></div>
            <ConnectionStatus/>
        </nav>
    }
}

#[component]
fn ConnectionStatus() -> impl IntoView {
    let conn_state = expect_context::<ConnectionState>();

    view! {
        <div class="flex items-center gap-2 text-xs">
            <div class=move || {
                if conn_state.connected.get() {
                    "w-2 h-2 rounded-full bg-success"
                } else {
                    "w-2 h-2 rounded-full bg-error"
                }
            }></div>
            <span class="text-muted">
                {move || if conn_state.connected.get() { "Connected" } else { "Disconnected" }}
            </span>
        </div>
    }
}

#[component]
pub fn NotFound() -> impl IntoView {
    view! {
        <Layout>
            <div class="max-w-4xl mx-auto py-12 text-center">
                <h1 class="text-4xl font-bold text-error mb-4">"404"</h1>
                <p class="text-muted mb-8">"Page not found"</p>
                <a href="/" class="text-accent hover:underline">"Go home"</a>
            </div>
        </Layout>
    }
}
