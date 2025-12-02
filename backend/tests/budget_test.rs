// tests/budget_test.rs
// Budget Tracker Tests - Daily/Monthly limits, cost tracking, cache hit rates

use chrono::Utc;
use mira_backend::budget::BudgetTracker;
use sqlx::sqlite::SqlitePoolOptions;
use std::sync::Arc;

async fn setup_test_db() -> Arc<sqlx::SqlitePool> {
    let pool = SqlitePoolOptions::new()
        .connect(":memory:")
        .await
        .expect("Failed to create in-memory database");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");
    Arc::new(pool)
}

/// Create a test user in the database (required for foreign key constraints)
async fn create_test_user(pool: &sqlx::SqlitePool, user_id: &str) {
    let now = Utc::now().timestamp();
    sqlx::query(
        r#"
        INSERT INTO users (id, username, email, password_hash, created_at, updated_at)
        VALUES (?, ?, ?, 'test-hash', ?, ?)
        "#,
    )
    .bind(user_id)
    .bind(format!("user_{}", user_id))
    .bind(format!("{}@test.com", user_id))
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .expect("Failed to create test user");
}

#[tokio::test]
async fn test_budget_tracker_creation() {
    println!("\n=== Testing Budget Tracker Creation ===\n");
    let pool = setup_test_db().await;

    let tracker = BudgetTracker::new((*pool).clone(), 5.0, 150.0);

    assert_eq!(tracker.daily_limit(), 5.0);
    assert_eq!(tracker.monthly_limit(), 150.0);
    println!("+ Budget tracker created with daily=$5.00, monthly=$150.00");
}

#[tokio::test]
async fn test_record_request_updates_totals() {
    println!("\n=== Testing Record Request Updates Totals ===\n");
    let pool = setup_test_db().await;
    let tracker = BudgetTracker::new((*pool).clone(), 5.0, 150.0);
    let user_id = "test-user-001";
    create_test_user(&pool, user_id).await;

    println!("[1] Recording first request");
    tracker
        .record_request(
            user_id,
            None, // No operation_id needed for budget tests
            "gemini",
            "gemini-2.5-pro",
            Some("high"),
            1000,  // tokens_input
            500,   // tokens_output
            0.05,  // cost_usd
            false, // from_cache
        )
        .await
        .expect("Failed to record request");

    println!("[2] Recording second request");
    tracker
        .record_request(
            user_id,
            None,
            "gemini",
            "gemini-2.5-pro",
            Some("medium"),
            2000,
            1000,
            0.10,
            false,
        )
        .await
        .expect("Failed to record request");

    println!("[3] Verifying daily usage");
    let usage = tracker
        .get_daily_usage(user_id)
        .await
        .expect("Failed to get daily usage");

    assert_eq!(usage.total_requests, 2);
    assert!((usage.total_cost_usd - 0.15).abs() < 0.001);
    assert_eq!(usage.tokens_input, 3000);
    assert_eq!(usage.tokens_output, 1500);
    assert_eq!(usage.cached_requests, 0);
    assert_eq!(usage.cache_hit_rate, 0.0);
    println!("+ Daily usage: {} requests, ${:.4}", usage.total_requests, usage.total_cost_usd);
}

#[tokio::test]
async fn test_daily_limit_enforcement() {
    println!("\n=== Testing Daily Limit Enforcement ===\n");
    let pool = setup_test_db().await;
    let tracker = BudgetTracker::new((*pool).clone(), 0.10, 150.0); // Low daily limit
    let user_id = "test-user-002";
    create_test_user(&pool, user_id).await;

    println!("[1] Check limit before any requests");
    let can_proceed = tracker
        .check_daily_limit(user_id)
        .await
        .expect("Failed to check daily limit");
    assert!(can_proceed, "Should allow requests when under limit");

    println!("[2] Record request that exceeds daily limit");
    tracker
        .record_request(
            user_id,
            None,
            "gemini",
            "gemini-2.5-pro",
            None,
            5000,
            2500,
            0.15, // Over the $0.10 limit
            false,
        )
        .await
        .expect("Failed to record request");

    println!("[3] Check limit after exceeding");
    let can_proceed = tracker
        .check_daily_limit(user_id)
        .await
        .expect("Failed to check daily limit");
    assert!(!can_proceed, "Should block requests when over daily limit");
    println!("+ Daily limit enforcement working correctly");
}

#[tokio::test]
async fn test_monthly_limit_enforcement() {
    println!("\n=== Testing Monthly Limit Enforcement ===\n");
    let pool = setup_test_db().await;
    let tracker = BudgetTracker::new((*pool).clone(), 100.0, 0.50); // Low monthly limit
    let user_id = "test-user-003";
    create_test_user(&pool, user_id).await;

    println!("[1] Check limit before any requests");
    let can_proceed = tracker
        .check_monthly_limit(user_id)
        .await
        .expect("Failed to check monthly limit");
    assert!(can_proceed, "Should allow requests when under limit");

    println!("[2] Record multiple requests that exceed monthly limit");
    for _ in 0..3 {
        tracker
            .record_request(
                user_id,
                None,
                "gemini",
                "gemini-2.5-pro",
                None,
                1000,
                500,
                0.20, // 3 * $0.20 = $0.60 > $0.50 limit
                false,
            )
            .await
            .expect("Failed to record request");
    }

    println!("[3] Check limit after exceeding");
    let can_proceed = tracker
        .check_monthly_limit(user_id)
        .await
        .expect("Failed to check monthly limit");
    assert!(!can_proceed, "Should block requests when over monthly limit");
    println!("+ Monthly limit enforcement working correctly");
}

#[tokio::test]
async fn test_cache_hit_rate_tracking() {
    println!("\n=== Testing Cache Hit Rate Tracking ===\n");
    let pool = setup_test_db().await;
    let tracker = BudgetTracker::new((*pool).clone(), 100.0, 1000.0);
    let user_id = "test-user-004";
    create_test_user(&pool, user_id).await;

    println!("[1] Recording 2 cache misses");
    for _ in 0..2 {
        tracker
            .record_request(
                user_id,
                None,
                "gemini",
                "gemini-2.5-pro",
                None,
                1000,
                500,
                0.05,
                false, // not from cache
            )
            .await
            .expect("Failed to record request");
    }

    println!("[2] Recording 2 cache hits");
    for _ in 0..2 {
        tracker
            .record_request(
                user_id,
                None,
                "gemini",
                "gemini-2.5-pro",
                None,
                1000,
                500,
                0.0, // free - from cache
                true, // from cache
            )
            .await
            .expect("Failed to record request");
    }

    println!("[3] Verifying cache hit rate");
    let usage = tracker
        .get_daily_usage(user_id)
        .await
        .expect("Failed to get daily usage");

    assert_eq!(usage.total_requests, 4);
    assert_eq!(usage.cached_requests, 2);
    assert!((usage.cache_hit_rate - 0.5).abs() < 0.001);
    println!("+ Cache hit rate: {:.0}%", usage.cache_hit_rate * 100.0);
}

#[tokio::test]
async fn test_check_both_limits() {
    println!("\n=== Testing Check Both Limits ===\n");
    let pool = setup_test_db().await;
    let tracker = BudgetTracker::new((*pool).clone(), 5.0, 150.0);
    let user_id = "test-user-005";
    create_test_user(&pool, user_id).await;

    println!("[1] Check limits when under both");
    let result = tracker.check_limits(user_id, 0.05).await;
    assert!(result.is_ok(), "Should pass when under both limits");

    println!("[2] Record requests to exceed daily limit");
    tracker
        .record_request(
            user_id,
            None,
            "gemini",
            "gemini-2.5-pro",
            None,
            10000,
            5000,
            5.50, // Over daily limit
            false,
        )
        .await
        .expect("Failed to record request");

    println!("[3] Check limits when over daily");
    let result = tracker.check_limits(user_id, 0.05).await;
    assert!(result.is_err(), "Should fail when over daily limit");
    let err_msg = result.unwrap_err().to_string();
    assert!(err_msg.contains("Daily budget limit"), "Error should mention daily limit");
    println!("+ Combined limit check working correctly");
}

#[tokio::test]
async fn test_monthly_usage_aggregation() {
    println!("\n=== Testing Monthly Usage Aggregation ===\n");
    let pool = setup_test_db().await;
    let tracker = BudgetTracker::new((*pool).clone(), 100.0, 1000.0);
    let user_id = "test-user-006";
    create_test_user(&pool, user_id).await;

    println!("[1] Recording 5 requests");
    for i in 0..5 {
        tracker
            .record_request(
                user_id,
                None,
                "gemini",
                "gemini-2.5-pro",
                Some("high"),
                1000 * (i + 1) as i64,
                500 * (i + 1) as i64,
                0.10 * (i + 1) as f64, // 0.10, 0.20, 0.30, 0.40, 0.50 = 1.50 total
                i % 2 == 0, // Alternate cache hits
            )
            .await
            .expect("Failed to record request");
    }

    println!("[2] Verifying monthly usage");
    let usage = tracker
        .get_monthly_usage(user_id)
        .await
        .expect("Failed to get monthly usage");

    assert_eq!(usage.total_requests, 5);
    assert!((usage.total_cost_usd - 1.50).abs() < 0.001);
    // Tokens: 1000+2000+3000+4000+5000 = 15000 input, 500+1000+1500+2000+2500 = 7500 output
    assert_eq!(usage.tokens_input, 15000);
    assert_eq!(usage.tokens_output, 7500);
    // Cache hits: i=0, i=2, i=4 = 3 hits
    assert_eq!(usage.cached_requests, 3);
    assert!((usage.cache_hit_rate - 0.6).abs() < 0.001);
    println!(
        "+ Monthly usage: {} requests, ${:.2}, {:.0}% cache hit rate",
        usage.total_requests,
        usage.total_cost_usd,
        usage.cache_hit_rate * 100.0
    );
}
