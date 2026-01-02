// crates/mira-app/src/components/chat/code_block.rs
// Syntax highlighted code block with copy button

use leptos::prelude::*;

#[component]
pub fn CodeBlock(
    code: String,
    #[prop(optional)] language: Option<String>,
) -> impl IntoView {
    let (copied, set_copied) = signal(false);
    let code_clone = code.clone();

    let copy_code = move |_| {
        let code = code_clone.clone();
        #[cfg(target_arch = "wasm32")]
        {
            use wasm_bindgen_futures::spawn_local;
            spawn_local(async move {
                if let Some(window) = web_sys::window() {
                    let clipboard = window.navigator().clipboard();
                    let _ = wasm_bindgen_futures::JsFuture::from(
                        clipboard.write_text(&code)
                    ).await;
                    set_copied.set(true);
                    // Reset after 2 seconds
                    gloo_timers::future::TimeoutFuture::new(2000).await;
                    set_copied.set(false);
                }
            });
        }
    };

    let lang_display = language.clone().unwrap_or_else(|| "text".to_string());

    view! {
        <div class="code-block">
            <div class="code-block-header">
                <span class="code-block-lang">{lang_display}</span>
                <button
                    class=move || if copied.get() { "code-block-copy copied" } else { "code-block-copy" }
                    on:click=copy_code
                >
                    {move || if copied.get() { "Copied!" } else { "Copy" }}
                </button>
            </div>
            <pre><code>{code}</code></pre>
        </div>
    }
}
