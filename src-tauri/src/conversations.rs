use crate::auth::SessionStore;
use crate::db::get_pool;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use tauri::{command, State};

// ============================================
// TYPES
// ============================================

/// A message within a conversation
#[derive(Serialize, Deserialize, Debug, Clone, FromRow)]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    pub sender_id: String,
    pub content: String,
    pub timestamp: i64,
}

/// Conversation with additional details for display
#[derive(Serialize, Deserialize, Debug)]
pub struct ConversationWithDetails {
    pub conversation_id: String,
    pub conversation_type: String,
    pub name: Option<String>,
    pub other_user_id: Option<String>,
    pub other_user_nickname: Option<String>,
    pub last_message: Option<String>,
    pub last_message_time: Option<i64>,
    pub has_unread: bool,
}

/// Result for conversation operations
#[derive(Serialize)]
pub struct ConversationResult {
    pub success: bool,
    pub conversation_id: Option<String>,
    pub error: Option<String>,
}

/// Result for message operations
#[derive(Serialize)]
pub struct MessageResult {
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
// CONVERSATION COMMANDS
// ============================================

/// Get or create a DM conversation with another user
#[command]
pub async fn get_or_create_dm_conversation(
    other_user_id: String,
    session_store: State<'_, SessionStore>,
) -> Result<ConversationResult, String> {
    let user_id = get_user_id_from_store(&session_store)?;
    let pool = get_pool();

    // Validate other_user_id
    if uuid::Uuid::parse_str(&other_user_id).is_err() {
        return Ok(ConversationResult {
            success: false,
            conversation_id: None,
            error: Some("Invalid user ID".to_string()),
        });
    }

    if other_user_id == user_id {
        return Ok(ConversationResult {
            success: false,
            conversation_id: None,
            error: Some("Cannot create conversation with yourself".to_string()),
        });
    }

    // Use a transaction to prevent race conditions
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| format!("Failed to start transaction: {}", e))?;

    // Create canonical key for this DM pair (sorted for consistency)
    let dm_key = if user_id < other_user_id {
        format!("{}:{}", user_id, other_user_id)
    } else {
        format!("{}:{}", other_user_id, user_id)
    };

    // Check if a DM conversation already exists using the unique key
    let existing: Option<(String,)> = sqlx::query_as(
        "SELECT id::text FROM conversations 
         WHERE type = 'direct' AND dm_participant_key = $1
         FOR UPDATE"
    )
    .bind(&dm_key)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| format!("Database error: {}", e))?;

    if let Some((conversation_id,)) = existing {
        tx.commit()
            .await
            .map_err(|e| format!("Failed to commit transaction: {}", e))?;
        return Ok(ConversationResult {
            success: true,
            conversation_id: Some(conversation_id),
            error: None,
        });
    }

    // Create new conversation with a unique key to prevent duplicates
    // Try to insert - if duplicate key, fetch the existing conversation
    let insert_result: Result<(String,), _> = sqlx::query_as(
        "INSERT INTO conversations (type, dm_participant_key) VALUES ('direct', $1) 
         ON CONFLICT (dm_participant_key) WHERE type = 'direct' AND dm_participant_key IS NOT NULL
         DO NOTHING
         RETURNING id::text"
    )
    .bind(&dm_key)
    .fetch_one(&mut *tx)
    .await;

    let conversation_id = match insert_result {
        Ok((id,)) => id,
        Err(_) => {
            // Conflict occurred - fetch the existing conversation
            let existing: (String,) = sqlx::query_as(
                "SELECT id::text FROM conversations WHERE dm_participant_key = $1"
            )
            .bind(&dm_key)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| format!("Database error: {}", e))?;
            
            tx.commit()
                .await
                .map_err(|e| format!("Failed to commit transaction: {}", e))?;
            
            return Ok(ConversationResult {
                success: true,
                conversation_id: Some(existing.0),
                error: None,
            });
        }
    };

    // Add both participants
    sqlx::query(
        "INSERT INTO conversation_participants (conversation_id, user_id) VALUES ($1::uuid, $2), ($1::uuid, $3)"
    )
    .bind(&conversation_id)
    .bind(&user_id)
    .bind(&other_user_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| format!("Database error: {}", e))?;

    tx.commit()
        .await
        .map_err(|e| format!("Failed to commit transaction: {}", e))?;

    Ok(ConversationResult {
        success: true,
        conversation_id: Some(conversation_id),
        error: None,
    })
}

/// Get all conversations for the current user
#[command]
pub async fn get_conversations(
    session_store: State<'_, SessionStore>,
) -> Result<Vec<ConversationWithDetails>, String> {
    let user_id = get_user_id_from_store(&session_store)?;
    let pool = get_pool();

    // Complex query to get conversations with all details
    let rows: Vec<(
        String,           // conversation_id
        String,           // conversation_type
        Option<String>,   // name
        Option<String>,   // other_user_id
        Option<String>,   // other_user_nickname
        Option<String>,   // last_message
        Option<i64>,      // last_message_time
        bool,             // has_unread
    )> = sqlx::query_as(
        r#"
        SELECT 
            c.id::text as conversation_id,
            c.type as conversation_type,
            c.name,
            -- Get other user for DMs
            (SELECT cp2.user_id FROM conversation_participants cp2 
             WHERE cp2.conversation_id = c.id AND cp2.user_id != $1 LIMIT 1) as other_user_id,
            -- Get other user's nickname for DMs
            (SELECT p.nickname FROM profiles p 
             JOIN conversation_participants cp2 ON p.user_id = cp2.user_id
             WHERE cp2.conversation_id = c.id AND cp2.user_id != $1 LIMIT 1) as other_user_nickname,
            -- Get last message
            (SELECT m.content FROM messages m 
             WHERE m.conversation_id = c.id 
             ORDER BY m.timestamp DESC LIMIT 1) as last_message,
            -- Get last message time
            (SELECT m.timestamp FROM messages m 
             WHERE m.conversation_id = c.id 
             ORDER BY m.timestamp DESC LIMIT 1) as last_message_time,
            -- Check for unread messages
            COALESCE(
                (SELECT m.timestamp > COALESCE(EXTRACT(EPOCH FROM cp.last_read_at) * 1000, 0)
                 FROM messages m 
                 WHERE m.conversation_id = c.id 
                 ORDER BY m.timestamp DESC LIMIT 1),
                false
            ) as has_unread
        FROM conversations c
        JOIN conversation_participants cp ON c.id = cp.conversation_id
        WHERE cp.user_id = $1
        ORDER BY 
            (SELECT m.timestamp FROM messages m WHERE m.conversation_id = c.id ORDER BY m.timestamp DESC LIMIT 1) DESC NULLS LAST
        "#
    )
    .bind(&user_id)
    .fetch_all(pool.as_ref())
    .await
    .map_err(|e| format!("Database error: {}", e))?;

    let conversations: Vec<ConversationWithDetails> = rows
        .into_iter()
        .map(|(conversation_id, conversation_type, name, other_user_id, other_user_nickname, last_message, last_message_time, has_unread)| {
            ConversationWithDetails {
                conversation_id,
                conversation_type,
                name,
                other_user_id,
                other_user_nickname,
                last_message,
                last_message_time,
                has_unread,
            }
        })
        .collect();

    Ok(conversations)
}

/// Get messages for a specific conversation
#[command]
pub async fn get_messages(
    conversation_id: String,
    session_store: State<'_, SessionStore>,
) -> Result<Vec<Message>, String> {
    let user_id = get_user_id_from_store(&session_store)?;
    let pool = get_pool();

    if uuid::Uuid::parse_str(&conversation_id).is_err() {
        return Err("Invalid conversation ID".to_string());
    }

    // Verify user is a participant
    let participant: Option<(String,)> = sqlx::query_as(
        "SELECT user_id FROM conversation_participants WHERE conversation_id = $1::uuid AND user_id = $2"
    )
    .bind(&conversation_id)
    .bind(&user_id)
    .fetch_optional(pool.as_ref())
    .await
    .map_err(|e| format!("Database error: {}", e))?;

    if participant.is_none() {
        return Err("You are not a participant in this conversation".to_string());
    }

    let messages: Vec<Message> = sqlx::query_as(
        "SELECT id::text, conversation_id::text, sender_id, content, timestamp 
         FROM messages 
         WHERE conversation_id = $1::uuid 
         ORDER BY timestamp ASC"
    )
    .bind(&conversation_id)
    .fetch_all(pool.as_ref())
    .await
    .map_err(|e| format!("Database error: {}", e))?;

    Ok(messages)
}

/// Send a message to a conversation
#[command]
pub async fn send_message(
    conversation_id: String,
    content: String,
    session_store: State<'_, SessionStore>,
) -> Result<MessageResult, String> {
    let sender_id = get_user_id_from_store(&session_store)?;
    let pool = get_pool();

    // Validation
    if content.trim().is_empty() {
        return Ok(MessageResult {
            success: false,
            error: Some("Message content cannot be empty".to_string()),
        });
    }

    if content.len() > 5000 {
        return Ok(MessageResult {
            success: false,
            error: Some("Message content too long (max 5000 characters)".to_string()),
        });
    }

    if uuid::Uuid::parse_str(&conversation_id).is_err() {
        return Ok(MessageResult {
            success: false,
            error: Some("Invalid conversation ID".to_string()),
        });
    }

    // Verify user is a participant
    let participant: Option<(String,)> = sqlx::query_as(
        "SELECT user_id FROM conversation_participants WHERE conversation_id = $1::uuid AND user_id = $2"
    )
    .bind(&conversation_id)
    .bind(&sender_id)
    .fetch_optional(pool.as_ref())
    .await
    .map_err(|e| format!("Database error: {}", e))?;

    if participant.is_none() {
        return Ok(MessageResult {
            success: false,
            error: Some("You are not a participant in this conversation".to_string()),
        });
    }

    let timestamp = chrono::Utc::now().timestamp_millis();

    let result = sqlx::query(
        "INSERT INTO messages (conversation_id, sender_id, content, timestamp) VALUES ($1::uuid, $2, $3, $4)"
    )
    .bind(&conversation_id)
    .bind(&sender_id)
    .bind(content.trim())
    .bind(timestamp)
    .execute(pool.as_ref())
    .await;

    // Update conversation's updated_at
    let _ = sqlx::query("UPDATE conversations SET updated_at = NOW() WHERE id = $1::uuid")
        .bind(&conversation_id)
        .execute(pool.as_ref())
        .await;

    match result {
        Ok(_) => Ok(MessageResult {
            success: true,
            error: None,
        }),
        Err(e) => Ok(MessageResult {
            success: false,
            error: Some(format!("Failed to send message: {}", e)),
        }),
    }
}

/// Mark conversation as read (update last_read_at)
#[command]
pub async fn mark_conversation_read(
    conversation_id: String,
    session_store: State<'_, SessionStore>,
) -> Result<MessageResult, String> {
    let user_id = get_user_id_from_store(&session_store)?;
    let pool = get_pool();

    if uuid::Uuid::parse_str(&conversation_id).is_err() {
        return Ok(MessageResult {
            success: false,
            error: Some("Invalid conversation ID".to_string()),
        });
    }

    let result = sqlx::query(
        "UPDATE conversation_participants SET last_read_at = NOW() WHERE conversation_id = $1::uuid AND user_id = $2"
    )
    .bind(&conversation_id)
    .bind(&user_id)
    .execute(pool.as_ref())
    .await;

    match result {
        Ok(_) => Ok(MessageResult {
            success: true,
            error: None,
        }),
        Err(_) => Ok(MessageResult {
            success: false,
            error: Some("Failed to mark conversation as read".to_string()),
        }),
    }
}