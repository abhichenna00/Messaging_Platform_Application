use crate::config::{cognito_client_id, cognito_user_pool_id, aws_region};
use aws_sdk_cognitoidentityprovider::{
    Client as CognitoClient,
    types::{AuthFlowType, AttributeType},
};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{command, State};

/// Represents a user session stored securely on the backend
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    pub id_token: String,
    pub user_id: String,
    pub email: String,
    pub expires_at: i64,
}

/// Thread-safe session storage
pub struct SessionStore {
    pub session: Mutex<Option<Session>>,
}

impl Default for SessionStore {
    fn default() -> Self {
        Self {
            session: Mutex::new(None),
        }
    }
}

/// Public session info returned to frontend (no sensitive tokens)
#[derive(Serialize)]
pub struct PublicSessionInfo {
    pub user_id: String,
    pub email: String,
    pub is_authenticated: bool,
}

/// Result returned to frontend for auth operations
#[derive(Serialize)]
pub struct AuthResult {
    pub success: bool,
    pub error: Option<String>,
    pub user_id: Option<String>,
    pub needs_confirmation: bool,
}

/// Create Cognito client
async fn create_cognito_client() -> CognitoClient {
    let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .region(aws_config::Region::new(aws_region()))
        .load()
        .await;
    CognitoClient::new(&config)
}

/// Tauri command to sign in with email and password
#[command]
pub async fn sign_in(
    email: String,
    password: String,
    session_store: State<'_, SessionStore>,
) -> Result<AuthResult, String> {
    // Input validation
    if email.trim().is_empty() {
        return Ok(AuthResult {
            success: false,
            error: Some("Email is required".to_string()),
            user_id: None,
            needs_confirmation: false,
        });
    }

    if password.is_empty() {
        return Ok(AuthResult {
            success: false,
            error: Some("Password is required".to_string()),
            user_id: None,
            needs_confirmation: false,
        });
    }

    let client = create_cognito_client().await;

    let result = client
        .initiate_auth()
        .auth_flow(AuthFlowType::UserPasswordAuth)
        .client_id(cognito_client_id())
        .auth_parameters("USERNAME", email.trim())
        .auth_parameters("PASSWORD", &password)
        .send()
        .await;

    match result {
        Ok(response) => {
            if let Some(auth_result) = response.authentication_result() {
                let access_token = auth_result.access_token().unwrap_or_default().to_string();
                let refresh_token = auth_result.refresh_token().unwrap_or_default().to_string();
                let id_token = auth_result.id_token().unwrap_or_default().to_string();
                let expires_in = auth_result.expires_in() as i64;

                // Decode user info from ID token (JWT)
                let (user_id, user_email) = decode_id_token(&id_token);

                let expires_at = chrono::Utc::now().timestamp() + expires_in;

                let session = Session {
                    access_token,
                    refresh_token,
                    id_token,
                    user_id: user_id.clone(),
                    email: user_email,
                    expires_at,
                };

                let mut store = session_store.session.lock().map_err(|e| e.to_string())?;
                *store = Some(session);

                Ok(AuthResult {
                    success: true,
                    error: None,
                    user_id: Some(user_id),
                    needs_confirmation: false,
                })
            } else {
                Ok(AuthResult {
                    success: false,
                    error: Some("No authentication result".to_string()),
                    user_id: None,
                    needs_confirmation: false,
                })
            }
        }
        Err(e) => {
            let error_message = match e.into_service_error() {
                err if err.is_not_authorized_exception() => "Invalid email or password".to_string(),
                err if err.is_user_not_found_exception() => "User not found".to_string(),
                err if err.is_user_not_confirmed_exception() => {
                    return Ok(AuthResult {
                        success: false,
                        error: Some("Please confirm your email first".to_string()),
                        user_id: None,
                        needs_confirmation: true,
                    });
                }
                err => format!("Authentication failed: {:?}", err),
            };

            Ok(AuthResult {
                success: false,
                error: Some(error_message),
                user_id: None,
                needs_confirmation: false,
            })
        }
    }
}

/// Tauri command to sign up with email and password
#[command]
pub async fn sign_up(
    email: String,
    password: String,
    phone: Option<String>,
    session_store: State<'_, SessionStore>,
) -> Result<AuthResult, String> {
    // Input validation
    if email.trim().is_empty() {
        return Ok(AuthResult {
            success: false,
            error: Some("Email is required".to_string()),
            user_id: None,
            needs_confirmation: false,
        });
    }

    if password.len() < 8 {
        return Ok(AuthResult {
            success: false,
            error: Some("Password must be at least 8 characters".to_string()),
            user_id: None,
            needs_confirmation: false,
        });
    }

    let client = create_cognito_client().await;

    // Build user attributes
    let mut attributes = vec![
        AttributeType::builder()
            .name("email")
            .value(email.trim())
            .build()
            .unwrap(),
    ];

    if let Some(ref phone_number) = phone {
        if !phone_number.trim().is_empty() {
            attributes.push(
                AttributeType::builder()
                    .name("phone_number")
                    .value(phone_number.trim())
                    .build()
                    .unwrap(),
            );
        }
    }

    let result = client
        .sign_up()
        .client_id(cognito_client_id())
        .username(email.trim())
        .password(&password)
        .set_user_attributes(Some(attributes))
        .send()
        .await;

    match result {
        Ok(response) => {
            let user_id = response.user_sub().to_string();
            let confirmed = response.user_confirmed();

            if confirmed {
                // Auto-confirmed, sign them in
                return sign_in(email, password, session_store).await;
            }

            Ok(AuthResult {
                success: true,
                error: None,
                user_id: Some(user_id),
                needs_confirmation: true,
            })
        }
        Err(e) => {
            let error_message = match e.into_service_error() {
                err if err.is_username_exists_exception() => "An account with this email already exists".to_string(),
                err if err.is_invalid_password_exception() => "Password does not meet requirements".to_string(),
                err if err.is_invalid_parameter_exception() => {
                    // Show Cognito's actual message (could be email, phone, etc.)
                    err.meta().message().unwrap_or("Invalid parameter").to_string()
                }
                err => format!("Signup failed: {:?}", err),
            };

            Ok(AuthResult {
                success: false,
                error: Some(error_message),
                user_id: None,
                needs_confirmation: false,
            })
        }
    }
}

/// Confirm signup with verification code
#[command]
pub async fn confirm_sign_up(
    email: String,
    code: String,
) -> Result<AuthResult, String> {
    let client = create_cognito_client().await;

    let result = client
        .confirm_sign_up()
        .client_id(cognito_client_id())
        .username(email.trim())
        .confirmation_code(&code)
        .send()
        .await;

    match result {
        Ok(_) => Ok(AuthResult {
            success: true,
            error: None,
            user_id: None,
            needs_confirmation: false,
        }),
        Err(e) => {
            let error_message = match e.into_service_error() {
                err if err.is_code_mismatch_exception() => "Invalid verification code".to_string(),
                err if err.is_expired_code_exception() => "Verification code has expired".to_string(),
                err => format!("Confirmation failed: {:?}", err),
            };

            Ok(AuthResult {
                success: false,
                error: Some(error_message),
                user_id: None,
                needs_confirmation: true,
            })
        }
    }
}

/// Tauri command to sign out and clear the session
#[command]
pub async fn sign_out(session_store: State<'_, SessionStore>) -> Result<bool, String> {
    let mut store = session_store.session.lock().map_err(|e| e.to_string())?;
    *store = None;
    Ok(true)
}

/// Tauri command to get current session info (without exposing tokens)
#[command]
pub async fn get_session(
    session_store: State<'_, SessionStore>,
) -> Result<Option<PublicSessionInfo>, String> {
    let store = session_store.session.lock().map_err(|e| e.to_string())?;

    match &*store {
        Some(session) => {
            if chrono::Utc::now().timestamp() >= session.expires_at {
                Ok(None)
            } else {
                Ok(Some(PublicSessionInfo {
                    user_id: session.user_id.clone(),
                    email: session.email.clone(),
                    is_authenticated: true,
                }))
            }
        }
        None => Ok(None),
    }
}

/// Tauri command to get the auth token for API calls
#[command]
pub async fn get_auth_token(
    session_store: State<'_, SessionStore>,
) -> Result<Option<String>, String> {
    let store = session_store.session.lock().map_err(|e| e.to_string())?;

    match &*store {
        Some(session) => {
            if chrono::Utc::now().timestamp() >= session.expires_at {
                Ok(None)
            } else {
                Ok(Some(session.access_token.clone()))
            }
        }
        None => Ok(None),
    }
}

/// Tauri command to get user ID from current session
#[command]
pub async fn get_user_id(
    session_store: State<'_, SessionStore>,
) -> Result<Option<String>, String> {
    let store = session_store.session.lock().map_err(|e| e.to_string())?;

    match &*store {
        Some(session) => {
            if chrono::Utc::now().timestamp() >= session.expires_at {
                Ok(None)
            } else {
                Ok(Some(session.user_id.clone()))
            }
        }
        None => Ok(None),
    }
}

/// Tauri command to refresh the session token
#[command]
pub async fn refresh_session(session_store: State<'_, SessionStore>) -> Result<bool, String> {
    let refresh_token = {
        let store = session_store.session.lock().map_err(|e| e.to_string())?;
        match &*store {
            Some(session) => session.refresh_token.clone(),
            None => return Ok(false),
        }
    };

    let client = create_cognito_client().await;

    let result = client
        .initiate_auth()
        .auth_flow(AuthFlowType::RefreshTokenAuth)
        .client_id(cognito_client_id())
        .auth_parameters("REFRESH_TOKEN", &refresh_token)
        .send()
        .await;

    match result {
        Ok(response) => {
            if let Some(auth_result) = response.authentication_result() {
                let access_token = auth_result.access_token().unwrap_or_default().to_string();
                let id_token = auth_result.id_token().unwrap_or_default().to_string();
                let expires_in = auth_result.expires_in() as i64;

                let (user_id, user_email) = decode_id_token(&id_token);
                let expires_at = chrono::Utc::now().timestamp() + expires_in;

                let mut store = session_store.session.lock().map_err(|e| e.to_string())?;
                if let Some(session) = store.as_mut() {
                    session.access_token = access_token;
                    session.id_token = id_token;
                    session.user_id = user_id;
                    session.email = user_email;
                    session.expires_at = expires_at;
                }

                Ok(true)
            } else {
                Ok(false)
            }
        }
        Err(_) => {
            let mut store = session_store.session.lock().map_err(|e| e.to_string())?;
            *store = None;
            Ok(false)
        }
    }
}

/// Decode user info from Cognito ID token (JWT)
fn decode_id_token(id_token: &str) -> (String, String) {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    
    // JWT has 3 parts separated by dots: header.payload.signature
    let parts: Vec<&str> = id_token.split('.').collect();
    if parts.len() != 3 {
        return (String::new(), String::new());
    }

    // Decode the payload (second part)
    if let Ok(decoded) = URL_SAFE_NO_PAD.decode(parts[1]) {
        if let Ok(payload) = String::from_utf8(decoded) {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&payload) {
                let user_id = json["sub"].as_str().unwrap_or_default().to_string();
                let email = json["email"].as_str().unwrap_or_default().to_string();
                return (user_id, email);
            }
        }
    }

    (String::new(), String::new())
}

/// Sync OAuth session (for Google sign-in via hosted UI)
#[command]
pub async fn sync_oauth_session(
    access_token: String,
    refresh_token: String,
    id_token: String,
    user_id: String,
    email: String,
    expires_at: i64,
    session_store: State<'_, SessionStore>,
) -> Result<bool, String> {
    if access_token.is_empty() {
        return Err("Access token is required".to_string());
    }

    if user_id.is_empty() {
        return Err("User ID is required".to_string());
    }

    let session = Session {
        access_token,
        refresh_token,
        id_token,
        user_id,
        email,
        expires_at,
    };

    let mut store = session_store.session.lock().map_err(|e| e.to_string())?;
    *store = Some(session);

    Ok(true)
}

/// Tauri command to get WebSocket URL for realtime connections
#[command]
pub fn get_websocket_url() -> String {
    crate::config::websocket_url()
}