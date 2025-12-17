//! Stream processing for the REPL
//!
//! Handles incoming stream events, printing text deltas and collecting function calls.

use anyhow::Result;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::responses::{ResponsesResponse, StreamEvent};

use super::formatter::MarkdownFormatter;

/// Result of processing a stream
#[derive(Default)]
pub struct StreamResult {
    /// Collected function calls: (name, call_id, arguments)
    pub function_calls: Vec<(String, String, String)>,
    /// Final response with usage stats
    pub final_response: Option<ResponsesResponse>,
}

/// Process a stream of events, printing text and collecting function calls
///
/// Returns (result, was_cancelled, accumulated_text)
pub async fn process_stream(
    rx: &mut mpsc::Receiver<StreamEvent>,
    cancelled: &Arc<AtomicBool>,
) -> Result<(StreamResult, bool, String)> {
    let mut result = StreamResult::default();
    let mut printed_newline_before = false;
    let mut printed_any_text = false;
    let mut formatter = MarkdownFormatter::new();
    let mut accumulated_text = String::new();

    loop {
        // Check for cancellation
        if cancelled.load(Ordering::SeqCst) {
            // Flush formatter and reset colors
            let remaining = formatter.flush();
            if !remaining.is_empty() {
                print!("{}", remaining);
            }
            print!("\x1b[0m"); // Reset any pending colors
            if printed_any_text {
                println!();
            }
            println!("\n  [cancelled]");
            return Ok((result, true, accumulated_text));
        }

        // Use select! to allow cancellation checks even if recv blocks
        tokio::select! {
            event = rx.recv() => {
                match event {
                    Some(StreamEvent::TextDelta(delta)) => {
                        // Print newline before first text
                        if !printed_newline_before {
                            println!();
                            printed_newline_before = true;
                        }

                        // Accumulate raw text for saving
                        accumulated_text.push_str(&delta);

                        // Format and print delta immediately
                        let formatted = formatter.process(&delta);
                        if !formatted.is_empty() {
                            print!("{}", formatted);
                            io::stdout().flush()?;
                        }
                        printed_any_text = true;
                    }
                    Some(StreamEvent::FunctionCallStart { name, call_id }) => {
                        result.function_calls.push((name, call_id, String::new()));
                    }
                    Some(StreamEvent::FunctionCallDelta { call_id, arguments_delta }) => {
                        // Accumulate arguments
                        if let Some(fc) = result.function_calls.iter_mut().find(|(_, id, _)| id == &call_id) {
                            fc.2.push_str(&arguments_delta);
                        }
                    }
                    Some(StreamEvent::FunctionCallDone { name, call_id, arguments }) => {
                        // Update with final arguments
                        if let Some(fc) = result.function_calls.iter_mut().find(|(_, id, _)| id == &call_id) {
                            fc.2 = arguments;
                        } else {
                            result.function_calls.push((name, call_id, arguments));
                        }
                    }
                    Some(StreamEvent::Done(response)) => {
                        result.final_response = Some(response);
                        break;
                    }
                    Some(StreamEvent::Error(e)) => {
                        eprintln!("\nStream error: {}", e);
                        break;
                    }
                    None => break,
                }
            }
            // Small timeout to allow cancellation checks
            _ = tokio::time::sleep(tokio::time::Duration::from_millis(50)) => {
                // Just loop around to check cancellation
            }
        }
    }

    // Flush any remaining formatted content
    let remaining = formatter.flush();
    if !remaining.is_empty() {
        print!("{}", remaining);
        io::stdout().flush()?;
    }

    // Print newline after text if we printed any
    if printed_any_text {
        println!();
        println!();
    }

    Ok((result, false, accumulated_text))
}
