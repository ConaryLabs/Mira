// tests/sprint2_features.rs

use mira_backend::memory::decay::{calculate_decayed_salience, DecayConfig, reinforce_memory};
use mira_backend::memory::types::{MemoryEntry, MemoryType};
use mira_backend::persona::PersonaOverlay;
use chrono::{Utc, Duration};
use tokio_tungstenite::connect_async;
use futures::{SinkExt, StreamExt};
use serde_json::json;

#[tokio::test]
#[ignore] // Requires running server with full WebSocket implementation
async fn test_persona_switching_websocket() {
    let url = "ws://localhost:8080/ws/chat";
    let (mut ws, _) = connect_async(url).await.expect("Failed to connect");
    
    // Send initial message
    let msg = json!({
        "type": "message",
        "content": "Hey Mira, how are you?",
        "persona": "Default"
    });
    ws.send(tokio_tungstenite::tungstenite::Message::Text(msg.to_string().into())).await.unwrap();
    
    // Get some response chunks
    for _ in 0..3 {
        if let Some(Ok(msg)) = ws.next().await {
            match msg {
                tokio_tungstenite::tungstenite::Message::Text(text) => {
                    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
                    println!("Got message type: {}", parsed["type"]);
                }
                _ => {}
            }
        }
    }
    
    // Wait for any chunks to finish
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    
    // Switch persona
    let switch_msg = json!({
        "type": "switch_persona",
        "persona": "Forbidden",
        "smooth_transition": true
    });
    ws.send(tokio_tungstenite::tungstenite::Message::Text(switch_msg.to_string().into())).await.unwrap();
    
    // Should get transition/update messages
    let mut found_update = false;
    for _ in 0..5 {
        if let Some(Ok(msg)) = ws.next().await {
            match msg {
                tokio_tungstenite::tungstenite::Message::Text(text) => {
                    let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
                    if parsed["type"] == "persona_update" {
                        assert_eq!(parsed["persona"], "forbidden");
                        found_update = true;
                        break;
                    }
                }
                _ => {}
            }
        }
    }
    assert!(found_update, "Did not receive persona_update message");
}

#[test]
fn test_memory_decay_over_time() {
    let config = DecayConfig::default();
    let now = Utc::now();
    
    let test_cases = vec![
        (Duration::hours(1), 9.0, MemoryType::Feeling, 8.5),   // Feelings decay slowly
        (Duration::hours(1), 9.0, MemoryType::Joke, 7.0),      // Jokes decay faster
        (Duration::days(7), 8.0, MemoryType::Promise, 7.3),    // Promises persist (adjusted threshold)
        (Duration::days(30), 5.0, MemoryType::Fact, 3.5),      // Facts fade moderately with gentler curve
    ];
    
    for (age, initial_salience, memory_type, expected_min) in test_cases {
        let memory = MemoryEntry {
            id: Some(1),
            session_id: "test".to_string(),
            role: "user".to_string(),
            content: "Test memory".to_string(),
            timestamp: now - age,
            embedding: None,
            salience: Some(initial_salience),
            tags: None,
            summary: None,
            memory_type: Some(memory_type.clone()),
            logprobs: None,
            moderation_flag: None,
            system_fingerprint: None,
        };
        
        let decayed = calculate_decayed_salience(&memory, &config, now);
        assert!(
            decayed >= expected_min,
            "{:?} memory decayed to {} (expected >= {})",
            memory_type, decayed, expected_min
        );
    }
}

#[test]
fn test_memory_reinforcement() {
    let config = DecayConfig::default();
    let mut memory = MemoryEntry {
        id: Some(1),
        session_id: "test".to_string(),
        role: "assistant".to_string(),
        content: "I'll always remember this moment".to_string(),
        timestamp: Utc::now() - Duration::days(3),
        embedding: None,
        salience: Some(6.0),
        tags: Some(vec!["emotional".to_string()]),
        summary: None,
        memory_type: Some(MemoryType::Feeling),
        logprobs: None,
        moderation_flag: None,
        system_fingerprint: None,
    };
    
    // Decay should reduce salience
    let decayed = calculate_decayed_salience(&memory, &config, Utc::now());
    assert!(decayed < 6.0);
    
    // Reinforcement should boost it back up
    reinforce_memory(&mut memory, &config, 8.0);
    assert!(memory.salience.unwrap() > 6.0);
    assert!(memory.tags.as_ref().unwrap().contains(&"reinforced".to_string()));
}

#[tokio::test]
#[ignore] // Requires running server with full WebSocket implementation
async fn test_memory_stats_websocket() {
    let url = "ws://localhost:8080/ws/chat";
    let (mut ws, _) = connect_async(url).await.expect("Failed to connect");
    
    // Request memory stats
    let stats_msg = json!({
        "type": "get_memory_stats",
        "session_id": null  // Use current session
    });
    ws.send(tokio_tungstenite::tungstenite::Message::Text(stats_msg.to_string().into())).await.unwrap();
    
    // Should get stats response
    if let Some(Ok(msg)) = ws.next().await {
        match msg {
            tokio_tungstenite::tungstenite::Message::Text(text) => {
                let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
                assert_eq!(parsed["type"], "memory_stats");
                assert!(parsed["total_memories"].is_number());
                assert!(parsed["avg_salience"].is_number());
            }
            _ => panic!("Expected memory stats"),
        }
    }
}

#[test]
fn test_persona_parsing() {
    // Standard personas
    assert_eq!("default".parse::<PersonaOverlay>().unwrap(), PersonaOverlay::Default);
    assert_eq!("forbidden".parse::<PersonaOverlay>().unwrap(), PersonaOverlay::Forbidden);
    assert_eq!("hallow".parse::<PersonaOverlay>().unwrap(), PersonaOverlay::Hallow);
    assert_eq!("haven".parse::<PersonaOverlay>().unwrap(), PersonaOverlay::Haven);
    
    // Invalid
    assert!("invalid".parse::<PersonaOverlay>().is_err());
}
