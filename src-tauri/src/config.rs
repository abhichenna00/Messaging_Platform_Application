// src-tauri/src/config.rs

use std::env;

// Database
pub fn database_url() -> String {
    env::var("DATABASE_URL").expect("DATABASE_URL must be set")
}

// AWS Region
pub fn aws_region() -> String {
    env::var("AWS_REGION").unwrap_or_else(|_| "us-east-2".to_string())
}

// S3
pub fn s3_bucket() -> String {
    env::var("S3_BUCKET").expect("S3_BUCKET must be set")
}

pub fn cloudfront_url() -> String {
    env::var("CLOUDFRONT_URL").expect("CLOUDFRONT_URL must be set")
}

// Cognito
pub fn cognito_user_pool_id() -> String {
    env::var("COGNITO_USER_POOL_ID").expect("COGNITO_USER_POOL_ID must be set")
}

pub fn cognito_client_id() -> String {
    env::var("COGNITO_CLIENT_ID").expect("COGNITO_CLIENT_ID must be set")
}

// WebSocket
pub fn websocket_url() -> String {
    env::var("WEBSOCKET_URL").expect("WEBSOCKET_URL must be set")
}