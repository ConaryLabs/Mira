// tests/test_memory_decay.rs

use mira_backend::memory::decay::{
    DecayConfig, calculate_decayed_salience, reinforce_memory
};
use mira_backend::memory::types::{MemoryEntry, MemoryType};
use chrono::{Duration, Utc};

#[test]
fn test_emotional_memories_decay_slower() {
    let config = DecayConfig::default();
    let now = Utc::now();
    let one_day_ago = now - Duration::days(1);
    
    let feeling_memory = MemoryEntry {
        id: Some(1),
        session_id: "test".to_string(),
        role: "user".to_string(),
        content: "I love you".to_string(),
        timestamp: one_day_ago,
        embedding: None,
        salience: Some(9.0),
        tags: None,
        summary: None,
        memory_type: Some(MemoryType::Feeling),
        logprobs: None,
        moderation_flag: None,
        system_fingerprint: None,
    };
    
    let mut joke_memory = feeling_memory.clone();
    joke_memory.memory_type = Some(MemoryType::Joke);
    
    let feeling_decay = calculate_decayed_salience(&feeling_memory, &config, now);
    let joke_decay = calculate_decayed_salience(&joke_memory, &config, now);
    
    assert!(feeling_decay > joke_decay, "Feelings should decay slower than jokes");
    println!("✅ Emotional decay test passed: feeling={:.2}, joke={:.2}", feeling_decay, joke_decay);
}

#[test]
fn test_reinforcement_increases_salience() {
    let config = DecayConfig::default();
    let mut memory = MemoryEntry {
        id: Some(1),
        session_id: "test".to_string(),
        role: "user".to_string(),
        content: "Remember this".to_string(),
        timestamp: Utc::now(),
        embedding: None,
        salience: Some(5.0),
        tags: Some(vec![]),
        summary: None,
        memory_type: Some(MemoryType::Fact),
        logprobs: None,
        moderation_flag: None,
        system_fingerprint: None,
    };
    
    let original_salience = memory.salience.unwrap();
    reinforce_memory(&mut memory, &config, 7.0);
    
    assert!(memory.salience.unwrap() > original_salience, "Reinforcement should increase salience");
    assert!(memory.tags.as_ref().unwrap().contains(&"reinforced".to_string()));
    println!("✅ Reinforcement test passed: {:.2} -> {:.2}", original_salience, memory.salience.unwrap());
}

#[test]
fn test_promise_memories_are_persistent() {
    let config = DecayConfig::default();
    let now = Utc::now();
    let one_week_ago = now - Duration::days(7);
    
    let promise_memory = MemoryEntry {
        id: Some(1),
        session_id: "test".to_string(),
        role: "assistant".to_string(),
        content: "I promise to remember this".to_string(),
        timestamp: one_week_ago,
        embedding: None,
        salience: Some(7.0),
        tags: None,
        summary: None,
        memory_type: Some(MemoryType::Promise),
        logprobs: None,
        moderation_flag: None,
        system_fingerprint: None,
    };
    
    let decayed = calculate_decayed_salience(&promise_memory, &config, now);
    
    // Even after a week, promise should retain most of its salience
    assert!(decayed > 6.0, "Promises should decay very slowly");
    println!("✅ Promise persistence test passed: original=7.0, after 1 week={:.2}", decayed);
}
