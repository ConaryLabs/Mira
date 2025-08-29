// tests/verify_tokenizer.rs
// A standalone test to diagnose issues with loading tokenizer.json

use anyhow::Result;
use tokenizers::Tokenizer;

#[test]
fn test_load_tokenizer_file() -> Result<()> {
    println!("--- Starting Tokenizer Verification Test ---");

    // This is the exact line of code that is failing in the other test.
    // Let's see if it works in isolation.
    println!("Attempting to load tokenizer.json from bytes...");
    let tokenizer_result =
        Tokenizer::from_bytes(include_bytes!("../tokenizer.json"));

    match tokenizer_result {
        Ok(_) => {
            println!("\n✅ SUCCESS: tokenizer.json was loaded and parsed successfully!");
            println!("This means the file is correct, and the issue might be related to the full test suite's setup.");
        }
        Err(e) => {
            println!("\n❌ FAILURE: Failed to load or parse tokenizer.json.");
            println!("This confirms the tokenizer.json file itself is the problem.");
            println!("\nError details: {}", e);
            // Force a panic to make the test failure clear.
            panic!("Tokenizer file is invalid or cannot be read.");
        }
    }

    Ok(())
}
