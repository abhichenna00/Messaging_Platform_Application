use crate::auth::SessionStore;
use crate::config::{s3_bucket, cloudfront_url, aws_region};
use crate::db::get_pool;
use aws_sdk_s3::Client as S3Client;
use aws_sdk_s3::primitives::ByteStream;
use base64::{engine::general_purpose::STANDARD, Engine};
use rand::seq::SliceRandom;
use rand::Rng;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use tauri::{command, State};

/// Valid status values
pub const VALID_STATUSES: [&str; 4] = ["online", "idle", "dnd", "offline"];

/// Word lists for placeholder profile generation
const ADJECTIVES: &[&str] = &[
    "Swift", "Clever", "Bright", "Bold", "Calm", "Daring", "Eager", "Fancy",
    "Gentle", "Happy", "Jolly", "Keen", "Lively", "Mighty", "Noble", "Peppy",
    "Quick", "Radiant", "Sunny", "Witty", "Zesty", "Cosmic", "Lucky", "Mystic",
    "Pixel", "Quantum", "Retro", "Stellar", "Turbo", "Ultra", "Velvet", "Warp",
    "Amber", "Azure", "Crimson", "Emerald", "Golden", "Indigo", "Jade", "Lunar",
    "Neon", "Onyx", "Pearl", "Ruby", "Silver", "Violet", "Crystal", "Shadow",
];

const NOUNS: &[&str] = &[
    "Panda", "Phoenix", "Dragon", "Wolf", "Falcon", "Tiger", "Bear", "Fox",
    "Hawk", "Lion", "Raven", "Shark", "Viper", "Eagle", "Panther", "Cobra",
    "Comet", "Nova", "Star", "Moon", "Spark", "Storm", "Wave", "Flame",
    "Frost", "Thunder", "Blaze", "Echo", "Drift", "Pulse", "Dash", "Flash",
    "Knight", "Ninja", "Pilot", "Ranger", "Scout", "Wizard", "Hunter", "Voyager",
    "Byte", "Cipher", "Glitch", "Matrix", "Nexus", "Pixel", "Proxy", "Vector",
];

/// Profile data returned to frontend
#[derive(Serialize, Deserialize, Debug, FromRow)]
pub struct ProfileData {
    pub username: String,
    pub nickname: String,
    pub avatar_url: Option<String>,
    pub status: Option<String>,
}

/// Result for profile operations
#[derive(Serialize)]
pub struct ProfileResult {
    pub success: bool,
    pub error: Option<String>,
}

/// Result for image upload operations
#[derive(Serialize)]
pub struct ImageUploadResult {
    pub success: bool,
    pub url: Option<String>,
    pub error: Option<String>,
}

/// Placeholder profile data for new users
#[derive(Serialize)]
pub struct PlaceholderProfile {
    pub username: String,
    pub nickname: String,
}

/// Profile data for message display and friend lists
#[derive(Serialize, Deserialize, Debug, FromRow)]
pub struct ProfileNickname {
    pub user_id: String,
    pub nickname: String,
    pub avatar_url: Option<String>,
    pub status: Option<String>,
}

/// Helper function to get user ID from session store
fn get_user_id_from_store(session_store: &SessionStore) -> Result<String, String> {
    let store = session_store
        .session
        .lock()
        .map_err(|e| format!("Failed to lock session: {}", e))?;

    match &*store {
        Some(session) => {
            if chrono::Utc::now().timestamp() >= session.expires_at {
                Err("Session expired. Please sign in again.".to_string())
            } else {
                Ok(session.user_id.clone())
            }
        }
        None => Err("Not authenticated. Please sign in.".to_string()),
    }
}

/// Create S3 client
async fn create_s3_client() -> S3Client {
    let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_config::Region::new(aws_region()))
        .load()
        .await;
    S3Client::new(&config)
}

/// Tauri command to generate placeholder profile data for new users
#[command]
pub fn generate_placeholder_profile() -> PlaceholderProfile {
    let mut rng = rand::thread_rng();

    let adjective = ADJECTIVES.choose(&mut rng).unwrap_or(&"Cool");
    let noun = NOUNS.choose(&mut rng).unwrap_or(&"User");
    let number: u16 = rng.gen_range(1000..9999);

    let nickname = format!("{} {}", adjective, noun);
    let username = format!("{}_{}{}", adjective.to_lowercase(), noun.to_lowercase(), number);

    PlaceholderProfile { username, nickname }
}

/// Tauri command to upload a profile image to S3
#[command]
pub async fn upload_profile_image(
    image_data: String,
    file_name: String,
    content_type: String,
    session_store: State<'_, SessionStore>,
) -> Result<ImageUploadResult, String> {
    let user_id = get_user_id_from_store(&session_store)?;

    // Decode base64 image data
    let image_bytes = STANDARD
        .decode(&image_data)
        .map_err(|e| format!("Failed to decode image data: {}", e))?;

    // Validate image size (max 5MB)
    if image_bytes.len() > 5 * 1024 * 1024 {
        return Ok(ImageUploadResult {
            success: false,
            url: None,
            error: Some("Image must be less than 5MB".to_string()),
        });
    }

    let s3_client = create_s3_client().await;

    // Create unique file path: avatars/{user_id}/{filename}
    let key = format!("avatars/{}/{}", user_id, file_name);

    let result = s3_client
        .put_object()
        .bucket(s3_bucket())
        .key(&key)
        .body(ByteStream::from(image_bytes))
        .content_type(&content_type)
        .send()
        .await;

    match result {
        Ok(_) => {
            // Construct CloudFront URL
            let public_url = format!("{}/{}", cloudfront_url(), key);

            Ok(ImageUploadResult {
                success: true,
                url: Some(public_url),
                error: None,
            })
        }
        Err(e) => Ok(ImageUploadResult {
            success: false,
            url: None,
            error: Some(format!("Failed to upload image: {}", e)),
        }),
    }
}

/// Tauri command to delete a profile image from S3
#[command]
pub async fn delete_profile_image(
    session_store: State<'_, SessionStore>,
) -> Result<ProfileResult, String> {
    let user_id = get_user_id_from_store(&session_store)?;

    let s3_client = create_s3_client().await;

    // List and delete all objects in the user's avatar folder
    let prefix = format!("avatars/{}/", user_id);

    let list_result = s3_client
        .list_objects_v2()
        .bucket(s3_bucket())
        .prefix(&prefix)
        .send()
        .await;

    match list_result {
        Ok(output) => {
            for obj in output.contents() {
                if let Some(key) = obj.key() {
                    let _ = s3_client
                        .delete_object()
                        .bucket(s3_bucket())
                        .key(key)
                        .send()
                        .await;
                }
            }
            Ok(ProfileResult {
                success: true,
                error: None,
            })
        }
        Err(e) => Ok(ProfileResult {
            success: false,
            error: Some(format!("Failed to delete image: {}", e)),
        }),
    }
}

/// Tauri command to check if user has a profile
#[command]
pub async fn check_profile_exists(session_store: State<'_, SessionStore>) -> Result<bool, String> {
    let user_id = get_user_id_from_store(&session_store)?;
    let pool = get_pool();

    let result: Option<(i32,)> = sqlx::query_as(
        "SELECT 1 FROM profiles WHERE user_id = $1 LIMIT 1"
    )
    .bind(&user_id)
    .fetch_optional(pool.as_ref())
    .await
    .map_err(|e| format!("Database error: {}", e))?;

    Ok(result.is_some())
}

/// Tauri command to get the current user's profile
#[command]
pub async fn get_profile(
    session_store: State<'_, SessionStore>,
) -> Result<Option<ProfileData>, String> {
    let user_id = get_user_id_from_store(&session_store)?;
    let pool = get_pool();

    let profile: Option<ProfileData> = sqlx::query_as(
        "SELECT username, nickname, avatar_url, status FROM profiles WHERE user_id = $1"
    )
    .bind(&user_id)
    .fetch_optional(pool.as_ref())
    .await
    .map_err(|e| format!("Database error: {}", e))?;

    Ok(profile)
}

/// Tauri command to create a new profile
#[command]
pub async fn create_profile(
    username: String,
    nickname: String,
    avatar_url: Option<String>,
    session_store: State<'_, SessionStore>,
) -> Result<ProfileResult, String> {
    let user_id = get_user_id_from_store(&session_store)?;
    let pool = get_pool();

    // Validate input
    if username.trim().is_empty() || nickname.trim().is_empty() {
        return Ok(ProfileResult {
            success: false,
            error: Some("Username and nickname are required".to_string()),
        });
    }

    // Check if username is taken
    let existing: Option<(String,)> = sqlx::query_as(
        "SELECT username FROM profiles WHERE username = $1 LIMIT 1"
    )
    .bind(username.trim())
    .fetch_optional(pool.as_ref())
    .await
    .map_err(|e| format!("Database error: {}", e))?;

    if existing.is_some() {
        return Ok(ProfileResult {
            success: false,
            error: Some("Username is already taken".to_string()),
        });
    }

    // Create profile
    let result = sqlx::query(
        "INSERT INTO profiles (user_id, username, nickname, avatar_url, status) VALUES ($1, $2, $3, $4, 'online')"
    )
    .bind(&user_id)
    .bind(username.trim())
    .bind(nickname.trim())
    .bind(&avatar_url)
    .execute(pool.as_ref())
    .await;

    match result {
        Ok(_) => Ok(ProfileResult {
            success: true,
            error: None,
        }),
        Err(e) => Ok(ProfileResult {
            success: false,
            error: Some(format!("Failed to create profile: {}", e)),
        }),
    }
}

/// Tauri command to get profiles by user IDs (for message display)
#[command]
pub async fn get_profiles_by_ids(
    user_ids: Vec<String>,
    session_store: State<'_, SessionStore>,
) -> Result<Vec<ProfileNickname>, String> {
    let _ = get_user_id_from_store(&session_store)?; // Verify authenticated
    let pool = get_pool();

    if user_ids.is_empty() {
        return Ok(vec![]);
    }

    // Validate all UUIDs
    for id in &user_ids {
        if uuid::Uuid::parse_str(id).is_err() {
            return Err(format!("Invalid user ID format: {}", id));
        }
    }

    let profiles: Vec<ProfileNickname> = sqlx::query_as(
        "SELECT user_id, nickname, avatar_url, status FROM profiles WHERE user_id = ANY($1)"
    )
    .bind(&user_ids)
    .fetch_all(pool.as_ref())
    .await
    .map_err(|e| format!("Database error: {}", e))?;

    Ok(profiles)
}

/// Tauri command to update an existing profile
#[command]
pub async fn update_profile(
    username: String,
    nickname: String,
    avatar_url: Option<String>,
    session_store: State<'_, SessionStore>,
) -> Result<ProfileResult, String> {
    let user_id = get_user_id_from_store(&session_store)?;
    let pool = get_pool();

    // Validate input
    if username.trim().is_empty() || nickname.trim().is_empty() {
        return Ok(ProfileResult {
            success: false,
            error: Some("Username and nickname are required".to_string()),
        });
    }

    // Check if username is taken by someone else
    let existing: Option<(String,)> = sqlx::query_as(
        "SELECT username FROM profiles WHERE username = $1 AND user_id != $2 LIMIT 1"
    )
    .bind(username.trim())
    .bind(&user_id)
    .fetch_optional(pool.as_ref())
    .await
    .map_err(|e| format!("Database error: {}", e))?;

    if existing.is_some() {
        return Ok(ProfileResult {
            success: false,
            error: Some("Username is already taken".to_string()),
        });
    }

    // Update profile
    let result = sqlx::query(
        "UPDATE profiles SET username = $1, nickname = $2, avatar_url = $3 WHERE user_id = $4"
    )
    .bind(username.trim())
    .bind(nickname.trim())
    .bind(&avatar_url)
    .bind(&user_id)
    .execute(pool.as_ref())
    .await;

    match result {
        Ok(_) => Ok(ProfileResult {
            success: true,
            error: None,
        }),
        Err(e) => Ok(ProfileResult {
            success: false,
            error: Some(format!("Failed to update profile: {}", e)),
        }),
    }
}

/// Tauri command to update user status
#[command]
pub async fn update_status(
    status: String,
    session_store: State<'_, SessionStore>,
) -> Result<ProfileResult, String> {
    let user_id = get_user_id_from_store(&session_store)?;
    let pool = get_pool();

    // Validate status value
    if !VALID_STATUSES.contains(&status.as_str()) {
        return Ok(ProfileResult {
            success: false,
            error: Some(format!(
                "Invalid status. Must be one of: {}",
                VALID_STATUSES.join(", ")
            )),
        });
    }

    let result = sqlx::query(
        "UPDATE profiles SET status = $1 WHERE user_id = $2"
    )
    .bind(&status)
    .bind(&user_id)
    .execute(pool.as_ref())
    .await;

    match result {
        Ok(_) => Ok(ProfileResult {
            success: true,
            error: None,
        }),
        Err(e) => Ok(ProfileResult {
            success: false,
            error: Some(format!("Failed to update status: {}", e)),
        }),
    }
}