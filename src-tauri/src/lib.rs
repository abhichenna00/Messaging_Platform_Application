// Module declarations
mod auth;
mod config;
mod conversations;
mod db;
mod friends;
mod profile;

// Re-export the Tauri commands so they can be used in main
pub use auth::{
    confirm_sign_up, get_auth_token, get_session, get_user_id, get_websocket_url,
    refresh_session, sign_in, sign_out, sign_up, sync_oauth_session, SessionStore,
};
pub use conversations::{
    get_conversations, get_messages, get_or_create_dm_conversation, mark_conversation_read,
    send_message,
};
pub use friends::{
    accept_friend_request, cancel_friend_request, decline_friend_request, get_friends,
    get_incoming_friend_requests, get_outgoing_friend_requests, remove_friend, send_friend_request,
};
pub use profile::{
    check_profile_exists, create_profile, delete_profile_image, generate_placeholder_profile,
    get_profile, get_profiles_by_ids, update_profile, update_status, upload_profile_image,
};

use db::init_db;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Load .env from project root (parent of src-tauri)
    let parent_env = std::path::Path::new("../.env");
    if parent_env.exists() {
        dotenvy::from_path(parent_env).ok();
    } else {
        dotenvy::dotenv().ok();
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            // When a second instance is launched, this runs in the first instance
            // argv contains the deep link URL
            println!("Single instance triggered with args: {:?}", argv);

            // Find the deep link URL in arguments
            if let Some(url) = argv.iter().find(|arg| arg.starts_with("cryptex://")) {
                println!("Deep link URL: {}", url);
                // Emit event to frontend
                use tauri::Emitter;
                let _ = app.emit("deep-link", url);
            }

            // Focus the existing window
            use tauri::Manager;
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_focus();
            }
        }))
        // Setup hook to initialize database
        .setup(|_app| {
            // Initialize database connection pool
            tauri::async_runtime::block_on(async {
                if let Err(e) = init_db().await {
                    eprintln!("Failed to initialize database: {}", e);
                    // You might want to show an error dialog here
                }
            });
            Ok(())
        })
        // Initialize the session store as managed state
        .manage(SessionStore::default())
        // Register all Tauri commands
        .invoke_handler(tauri::generate_handler![
            // Auth commands
            sign_in,
            sign_up,
            sign_out,
            get_session,
            get_auth_token,
            get_user_id,
            refresh_session,
            sync_oauth_session,
            confirm_sign_up,
            get_websocket_url,
            // Profile commands
            check_profile_exists,
            get_profile,
            get_profiles_by_ids,
            create_profile,
            update_profile,
            upload_profile_image,
            delete_profile_image,
            update_status,
            generate_placeholder_profile,
            // Friends commands
            send_friend_request,
            get_incoming_friend_requests,
            get_outgoing_friend_requests,
            accept_friend_request,
            decline_friend_request,
            cancel_friend_request,
            get_friends,
            remove_friend,
            // Conversation commands
            get_or_create_dm_conversation,
            get_conversations,
            get_messages,
            send_message,
            mark_conversation_read,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}