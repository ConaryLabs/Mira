// crates/mira-app/src/pages/home.rs
// Home page component

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use crate::api::fetch_health;
use crate::Layout;

#[component]
pub fn HomePage() -> impl IntoView {
    let (status, set_status) = signal("Checking...".to_string());

    // Check server health on mount
    Effect::new(move |_| {
        spawn_local(async move {
            match fetch_health().await {
                Ok(health) => set_status.set(format!("Connected - {}", health)),
                Err(e) => set_status.set(format!("Error: {}", e)),
            }
        });
    });

    view! {
        <Layout>
            <div class="max-w-4xl mx-auto py-12 text-center">
                <h1 class="text-4xl font-bold text-accent mb-4">"Mira Studio"</h1>
                <p class="text-muted mb-8">"Memory and Intelligence Layer for Claude Code"</p>

                <div class="grid grid-cols-2 gap-4 max-w-2xl mx-auto">
                    <HomeCard
                        title="Ghost Mode"
                        href="/ghost"
                        description="Real-time agent reasoning visualization"
                    />
                    <HomeCard
                        title="Memories"
                        href="/memories"
                        description="Semantic memory storage and search"
                    />
                    <HomeCard
                        title="Code Intel"
                        href="/code"
                        description="Code symbols and semantic search"
                    />
                    <HomeCard
                        title="Tasks"
                        href="/tasks"
                        description="Goals and task management"
                    />
                </div>

                <div class="mt-12 p-4 bg-card rounded-lg border border-border">
                    <h3 class="text-sm text-muted mb-2">"Server Status"</h3>
                    <p class=move || {
                        if status.get().starts_with("Connected") {
                            "text-success"
                        } else if status.get().starts_with("Error") {
                            "text-error"
                        } else {
                            "text-muted"
                        }
                    }>
                        {move || status.get()}
                    </p>
                </div>
            </div>
        </Layout>
    }
}

#[component]
fn HomeCard(
    title: &'static str,
    href: &'static str,
    description: &'static str,
) -> impl IntoView {
    view! {
        <a
            href=href
            class="block p-6 bg-card rounded-lg border border-border hover:border-accent transition-colors"
        >
            <h3 class="text-lg font-semibold mb-2">{title}</h3>
            <p class="text-sm text-muted">{description}</p>
        </a>
    }
}
