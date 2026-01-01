// crates/mira-app/src/pages/memories.rs
// Memories page - semantic memory storage and search

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;
use mira_types::MemoryFact;
use crate::api::{fetch_memories, recall_memories};
use crate::Layout;

#[component]
pub fn MemoriesPage() -> impl IntoView {
    let (memories, set_memories) = signal(Vec::<MemoryFact>::new());
    let (search_query, set_search_query) = signal(String::new());
    let (loading, set_loading) = signal(false);
    let (trigger_search, set_trigger_search) = signal(0u32);

    // Load memories on mount
    Effect::new(move |_| {
        spawn_local(async move {
            set_loading.set(true);
            if let Ok(mems) = fetch_memories().await {
                set_memories.set(mems);
            }
            set_loading.set(false);
        });
    });

    // Search effect - runs when trigger_search changes
    Effect::new(move |prev: Option<u32>| {
        let current = trigger_search.get();
        if prev.is_some() && current > 0 {
            let query = search_query.get();
            if !query.is_empty() {
                spawn_local(async move {
                    set_loading.set(true);
                    if let Ok(results) = recall_memories(&query).await {
                        set_memories.set(results);
                    }
                    set_loading.set(false);
                });
            }
        }
        current
    });

    view! {
        <Layout>
            <div class="max-w-4xl mx-auto">
                <h1 class="text-2xl font-bold mb-6">"Memories"</h1>

                // Search bar
                <div class="mb-6 flex gap-2">
                    <input
                        type="text"
                        placeholder="Search memories semantically..."
                        class="flex-1 p-3 bg-card border border-border rounded-lg focus:border-accent outline-none"
                        prop:value=move || search_query.get()
                        on:input=move |ev| {
                            set_search_query.set(event_target_value(&ev));
                        }
                        on:keydown=move |ev| {
                            if ev.key() == "Enter" {
                                set_trigger_search.update(|n| *n += 1);
                            }
                        }
                    />
                    <button
                        class="px-4 py-2 bg-accent text-background rounded-lg hover:opacity-90"
                        on:click=move |_| set_trigger_search.update(|n| *n += 1)
                    >
                        "Search"
                    </button>
                </div>

                // Loading state
                {move || loading.get().then(|| view! {
                    <div class="text-muted text-center py-4">"Loading..."</div>
                })}

                // Memory list
                <div class="space-y-4">
                    {move || {
                        let mems = memories.get();
                        if mems.is_empty() && !loading.get() {
                            view! {
                                <div class="text-muted italic text-center py-8">
                                    "No memories found"
                                </div>
                            }.into_any()
                        } else {
                            view! {
                                <For
                                    each=move || mems.clone()
                                    key=|m| m.id
                                    children=move |memory| {
                                        view! { <MemoryCard memory=memory/> }
                                    }
                                />
                            }.into_any()
                        }
                    }}
                </div>
            </div>
        </Layout>
    }
}

#[component]
fn MemoryCard(memory: MemoryFact) -> impl IntoView {
    view! {
        <div class="p-4 bg-card rounded-lg border border-border hover:border-accent transition-colors">
            <div class="flex gap-2 mb-2">
                <span class="badge badge-accent">{memory.fact_type.clone()}</span>
                {memory.category.clone().map(|cat| view! {
                    <span class="badge badge-muted">{cat}</span>
                })}
            </div>
            <p class="text-sm">{memory.content.clone()}</p>
            <div class="mt-2 text-xs text-muted">
                {memory.created_at.clone()}
            </div>
        </div>
    }
}
