use crate::auth::SessionStore;
use crate::db::get_pool;
use serde::{Deserialize, Serialize};
use tauri::{command, State};

// ============================================
// TYPES
// ============================================

#[derive(Serialize, Deserialize, Debug)]
pub struct FriendWithProfile {
    pub friend_id: String,
    pub username: String,
    pub nickname: String,
    pub created_at: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FriendRequestWithProfile {
    pub id: String,
    pub from_user_id: String,
    pub to_user_id: String,
    pub status: String,
    pub created_at: String,
    pub from_username: Option<String>,
    pub from_nickname: Option<String>,
    pub to_username: Option<String>,
    pub to_nickname: Option<String>,
}

#[derive(Serialize)]
pub struct FriendsResult {
    pub success: bool,
    pub error: Option<String>,
}

// ============================================
// HELPER FUNCTIONS
// ============================================

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

// ============================================
// FRIEND REQUEST COMMANDS
// ============================================

/// Send a friend request to another user by their username
#[command]
pub async fn send_friend_request(
    to_username: String,
    session_store: State<'_, SessionStore>,
) -> Result<FriendsResult, String> {
    let from_user_id = get_user_id_from_store(&session_store)?;
    let pool = get_pool();

    if to_username.trim().is_empty() {
        return Ok(FriendsResult {
            success: false,
            error: Some("Username is required".to_string()),
        });
    }

    // Look up user by username
    let target: Option<(String,)> = sqlx::query_as(
        "SELECT user_id FROM profiles WHERE username = $1"
    )
    .bind(to_username.trim())
    .fetch_optional(pool.as_ref())
    .await
    .map_err(|e| format!("Database error: {}", e))?;

    let to_user_id = match target {
        Some((id,)) => id,
        None => {
            return Ok(FriendsResult {
                success: false,
                error: Some("User not found".to_string()),
            });
        }
    };

    // Check if trying to add yourself
    if to_user_id == from_user_id {
        return Ok(FriendsResult {
            success: false,
            error: Some("You cannot send a friend request to yourself".to_string()),
        });
    }

    // Check if already friends
    let existing_friend: Option<(String,)> = sqlx::query_as(
        "SELECT id::text FROM friends WHERE user_id = $1 AND friend_id = $2"
    )
    .bind(&from_user_id)
    .bind(&to_user_id)
    .fetch_optional(pool.as_ref())
    .await
    .map_err(|e| format!("Database error: {}", e))?;

    if existing_friend.is_some() {
        return Ok(FriendsResult {
            success: false,
            error: Some("You are already friends with this user".to_string()),
        });
    }

    // Check for existing pending request (in either direction)
    let existing_request: Option<(String,)> = sqlx::query_as(
        "SELECT id::text FROM friend_requests 
         WHERE status = 'pending' 
         AND ((from_user_id = $1 AND to_user_id = $2) OR (from_user_id = $2 AND to_user_id = $1))"
    )
    .bind(&from_user_id)
    .bind(&to_user_id)
    .fetch_optional(pool.as_ref())
    .await
    .map_err(|e| format!("Database error: {}", e))?;

    if existing_request.is_some() {
        return Ok(FriendsResult {
            success: false,
            error: Some("A friend request already exists between you and this user".to_string()),
        });
    }

    // Send the friend request
    let result = sqlx::query(
        "INSERT INTO friend_requests (from_user_id, to_user_id, status) VALUES ($1, $2, 'pending')"
    )
    .bind(&from_user_id)
    .bind(&to_user_id)
    .execute(pool.as_ref())
    .await;

    match result {
        Ok(_) => Ok(FriendsResult {
            success: true,
            error: None,
        }),
        Err(e) => Ok(FriendsResult {
            success: false,
            error: Some(format!("Failed to send friend request: {}", e)),
        }),
    }
}

/// Get all pending friend requests received by the current user
#[command]
pub async fn get_incoming_friend_requests(
    session_store: State<'_, SessionStore>,
) -> Result<Vec<FriendRequestWithProfile>, String> {
    let user_id = get_user_id_from_store(&session_store)?;
    let pool = get_pool();

    // Join with profiles to get sender info
    let rows: Vec<(String, String, String, String, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT fr.id::text, fr.from_user_id, fr.to_user_id, fr.status, fr.created_at::text,
                p.username, p.nickname
         FROM friend_requests fr
         LEFT JOIN profiles p ON fr.from_user_id = p.user_id
         WHERE fr.to_user_id = $1 AND fr.status = 'pending'
         ORDER BY fr.created_at DESC"
    )
    .bind(&user_id)
    .fetch_all(pool.as_ref())
    .await
    .map_err(|e| format!("Database error: {}", e))?;

    let results: Vec<FriendRequestWithProfile> = rows
        .into_iter()
        .map(|(id, from_user_id, to_user_id, status, created_at, username, nickname)| {
            FriendRequestWithProfile {
                id,
                from_user_id,
                to_user_id,
                status,
                created_at,
                from_username: username,
                from_nickname: nickname,
                to_username: None,
                to_nickname: None,
            }
        })
        .collect();

    Ok(results)
}

/// Get all pending friend requests sent by the current user
#[command]
pub async fn get_outgoing_friend_requests(
    session_store: State<'_, SessionStore>,
) -> Result<Vec<FriendRequestWithProfile>, String> {
    let user_id = get_user_id_from_store(&session_store)?;
    let pool = get_pool();

    // Join with profiles to get recipient info
    let rows: Vec<(String, String, String, String, String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT fr.id::text, fr.from_user_id, fr.to_user_id, fr.status, fr.created_at::text,
                p.username, p.nickname
         FROM friend_requests fr
         LEFT JOIN profiles p ON fr.to_user_id = p.user_id
         WHERE fr.from_user_id = $1 AND fr.status = 'pending'
         ORDER BY fr.created_at DESC"
    )
    .bind(&user_id)
    .fetch_all(pool.as_ref())
    .await
    .map_err(|e| format!("Database error: {}", e))?;

    let results: Vec<FriendRequestWithProfile> = rows
        .into_iter()
        .map(|(id, from_user_id, to_user_id, status, created_at, username, nickname)| {
            FriendRequestWithProfile {
                id,
                from_user_id,
                to_user_id,
                status,
                created_at,
                from_username: None,
                from_nickname: None,
                to_username: username,
                to_nickname: nickname,
            }
        })
        .collect();

    Ok(results)
}

/// Accept a friend request
#[command]
pub async fn accept_friend_request(
    request_id: String,
    session_store: State<'_, SessionStore>,
) -> Result<FriendsResult, String> {
    let user_id = get_user_id_from_store(&session_store)?;
    let pool = get_pool();

    // Verify this request is for the current user and get the sender
    let request: Option<(String, String)> = sqlx::query_as(
        "SELECT from_user_id, to_user_id FROM friend_requests 
         WHERE id = $1::uuid AND to_user_id = $2 AND status = 'pending'"
    )
    .bind(&request_id)
    .bind(&user_id)
    .fetch_optional(pool.as_ref())
    .await
    .map_err(|e| format!("Database error: {}", e))?;

    let (from_user_id, to_user_id) = match request {
        Some(r) => r,
        None => {
            return Ok(FriendsResult {
                success: false,
                error: Some("Friend request not found".to_string()),
            });
        }
    };

    // Update request status
    sqlx::query("UPDATE friend_requests SET status = 'accepted' WHERE id = $1::uuid")
        .bind(&request_id)
        .execute(pool.as_ref())
        .await
        .map_err(|e| format!("Database error: {}", e))?;

    // Create bidirectional friendship
    sqlx::query("INSERT INTO friends (user_id, friend_id) VALUES ($1, $2), ($2, $1)")
        .bind(&from_user_id)
        .bind(&to_user_id)
        .execute(pool.as_ref())
        .await
        .map_err(|e| format!("Database error: {}", e))?;

    Ok(FriendsResult {
        success: true,
        error: None,
    })
}

/// Decline a friend request
#[command]
pub async fn decline_friend_request(
    request_id: String,
    session_store: State<'_, SessionStore>,
) -> Result<FriendsResult, String> {
    let user_id = get_user_id_from_store(&session_store)?;
    let pool = get_pool();

    let result = sqlx::query(
        "UPDATE friend_requests SET status = 'declined' WHERE id = $1::uuid AND to_user_id = $2"
    )
    .bind(&request_id)
    .bind(&user_id)
    .execute(pool.as_ref())
    .await;

    match result {
        Ok(_) => Ok(FriendsResult {
            success: true,
            error: None,
        }),
        Err(e) => Ok(FriendsResult {
            success: false,
            error: Some(format!("Failed to decline friend request: {}", e)),
        }),
    }
}

/// Cancel a sent friend request
#[command]
pub async fn cancel_friend_request(
    request_id: String,
    session_store: State<'_, SessionStore>,
) -> Result<FriendsResult, String> {
    let user_id = get_user_id_from_store(&session_store)?;
    let pool = get_pool();

    let result = sqlx::query(
        "DELETE FROM friend_requests WHERE id = $1::uuid AND from_user_id = $2"
    )
    .bind(&request_id)
    .bind(&user_id)
    .execute(pool.as_ref())
    .await;

    match result {
        Ok(_) => Ok(FriendsResult {
            success: true,
            error: None,
        }),
        Err(e) => Ok(FriendsResult {
            success: false,
            error: Some(format!("Failed to cancel friend request: {}", e)),
        }),
    }
}

// ============================================
// FRIENDS LIST COMMANDS
// ============================================

/// Get all friends for the current user
#[command]
pub async fn get_friends(
    session_store: State<'_, SessionStore>,
) -> Result<Vec<FriendWithProfile>, String> {
    let user_id = get_user_id_from_store(&session_store)?;
    let pool = get_pool();

    // Join with profiles to get friend info
    let rows: Vec<(String, String, String, String)> = sqlx::query_as(
        "SELECT f.friend_id, p.username, p.nickname, f.created_at::text
         FROM friends f
         JOIN profiles p ON f.friend_id = p.user_id
         WHERE f.user_id = $1
         ORDER BY p.nickname"
    )
    .bind(&user_id)
    .fetch_all(pool.as_ref())
    .await
    .map_err(|e| format!("Database error: {}", e))?;

    let results: Vec<FriendWithProfile> = rows
        .into_iter()
        .map(|(friend_id, username, nickname, created_at)| FriendWithProfile {
            friend_id,
            username,
            nickname,
            created_at,
        })
        .collect();

    Ok(results)
}

/// Remove a friend
#[command]
pub async fn remove_friend(
    friend_id: String,
    session_store: State<'_, SessionStore>,
) -> Result<FriendsResult, String> {
    let user_id = get_user_id_from_store(&session_store)?;
    let pool = get_pool();

    // Remove both directions of the friendship
    let result = sqlx::query(
        "DELETE FROM friends WHERE (user_id = $1 AND friend_id = $2) OR (user_id = $2 AND friend_id = $1)"
    )
    .bind(&user_id)
    .bind(&friend_id)
    .execute(pool.as_ref())
    .await;

    match result {
        Ok(_) => Ok(FriendsResult {
            success: true,
            error: None,
        }),
        Err(e) => Ok(FriendsResult {
            success: false,
            error: Some(format!("Failed to remove friend: {}", e)),
        }),
    }
}