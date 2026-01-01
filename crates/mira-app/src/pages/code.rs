// crates/mira-app/src/pages/code.rs
// Code Intelligence page - semantic code search

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;
use mira_types::CodeSearchResult;
use crate::api::search_code;
use crate::Layout;

#[component]
pub fn CodePage() -> impl IntoView {
    let (results, set_results) = signal(Vec::<CodeSearchResult>::new());
    let (search_query, set_search_query) = signal(String::new());
    let (loading, set_loading) = signal(false);
    let (trigger_search, set_trigger_search) = signal(0u32);

    // Search effect - runs when trigger_search changes
    Effect::new(move |prev: Option<u32>| {
        let current = trigger_search.get();
        if prev.is_some() && current > 0 {
            let query = search_query.get();
            if !query.is_empty() {
                spawn_local(async move {
                    set_loading.set(true);
                    if let Ok(res) = search_code(&query).await {
                        set_results.set(res);
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
                <h1 class="text-2xl font-bold mb-6">"Code Intelligence"</h1>

                // Search bar
                <div class="mb-6 flex gap-2">
                    <input
                        type="text"
                        placeholder="Semantic code search..."
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

                // Loading
                {move || loading.get().then(|| view! {
                    <div class="text-muted text-center py-4">"Searching..."</div>
                })}

                // Results
                <div class="space-y-4">
                    {move || {
                        let res = results.get();
                        if res.is_empty() && !loading.get() {
                            view! {
                                <div class="text-muted italic text-center py-8">
                                    "Enter a query to search code semantically"
                                </div>
                            }.into_any()
                        } else {
                            view! {
                                <For
                                    each=move || res.clone()
                                    key=|r| format!("{}:{}", r.file_path, r.line_number)
                                    children=move |result| {
                                        view! { <CodeResult result=result/> }
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
fn CodeResult(result: CodeSearchResult) -> impl IntoView {
    view! {
        <div class="p-4 bg-card rounded-lg border border-border">
            <div class="flex items-center gap-2 mb-2">
                <span class="font-mono text-accent">{result.file_path.clone()}</span>
                <span class="text-muted">":"</span>
                <span class="text-muted">{result.line_number}</span>
                {result.symbol_name.clone().map(|name| view! {
                    <span class="ml-2 badge badge-accent">{name}</span>
                })}
            </div>
            <pre class="text-sm font-mono bg-background p-2 rounded overflow-x-auto">
                {result.content.clone()}
            </pre>
        </div>
    }
}
