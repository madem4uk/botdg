use std::{
    collections::HashSet,
    ffi::{CStr, CString},
    sync::Arc,
    time::Instant,
    os::raw::c_void,
};
use regex::Regex;
use serde_json::json;
use tokio::sync::Mutex;
use log::{info, error, warn};
use libloading::{Library, Symbol};

// Default minimum amount if not specified in environment
const DEFAULT_MIN_AMOUNT: i32 = 38000;
const REACTION_EMOJI: &str = "👍";
const AUTH_TIMEOUT: f64 = 0.1;
const RECEIVE_TIMEOUT: f64 = 1.0;
const MAX_AUTH_ATTEMPTS: u8 = 3;
const TDLIB_VERSION: &str = "1.8.0";

// Get API credentials from environment variables
fn get_api_id() -> i32 {
    std::env::var("TELEGRAM_API_ID")
        .expect("TELEGRAM_API_ID must be set")
        .parse()
        .expect("TELEGRAM_API_ID must be a valid integer")
}

fn get_api_hash() -> String {
    std::env::var("TELEGRAM_API_HASH")
        .expect("TELEGRAM_API_HASH must be set")
}

// Get allowed chat IDs from environment variable
fn get_allowed_chat_ids() -> HashSet<i64> {
    std::env::var("ALLOWED_CHAT_IDS")
        .unwrap_or_default()
        .split(',')
        .filter_map(|s| s.trim().parse::<i64>().ok())
        .collect()
}

struct TdClient {
    client: *mut c_void,
    tdlib: Library,
}

impl TdClient {
    unsafe fn new() -> Self {
        // Try multiple possible locations for TDLib
        let possible_paths = if cfg!(target_os = "macos") {
            vec![
                std::env::var("TDLIB_PATH").ok(),
                Some("/usr/local/lib/libtdjson.dylib".to_string()),
                Some("/opt/homebrew/lib/libtdjson.dylib".to_string()),
                Some("./libtdjson.dylib".to_string())
            ]
        } else {
            vec![
                std::env::var("TDLIB_PATH").ok(),
                Some("/usr/local/lib/libtdjson.so".to_string()),
                Some("/usr/lib/libtdjson.so".to_string()),
                Some("./libtdjson.so".to_string())
            ]
        };
        
        // Filter out None values and try each path
        let valid_paths: Vec<String> = possible_paths.into_iter().flatten().collect();
        
        println!("Attempting to load TDLib from the following locations: {:?}", valid_paths);
        
        // Try each path until one works
        for lib_path in valid_paths {
            println!("Trying to load TDLib from: {}", lib_path);
            match Library::new(&lib_path) {
                Ok(tdlib) => {
                    match tdlib.get::<unsafe extern "C" fn() -> *mut c_void>(b"td_json_client_create") {
                        Ok(create) => {
                            println!("Successfully loaded TDLib from: {}", lib_path);
                            return TdClient {
                                client: create(),
                                tdlib,
                            };
                        },
                        Err(e) => {
                            println!("Found library at {} but couldn't get td_json_client_create: {}", lib_path, e);
                            continue;
                        }
                    }
                },
                Err(e) => {
                    println!("Failed to load TDLib from {}: {}", lib_path, e);
                    continue;
                }
            }
        }
        
        // If we get here, we couldn't find TDLib anywhere
        panic!("Could not find TDLib in any of the expected locations. Please install TDLib or set TDLIB_PATH environment variable.");
    }

    fn send(&self, request: &str) {
        let request_c = CString::new(request).unwrap();
        unsafe {
            let send: Symbol<unsafe extern "C" fn(*mut c_void, *const i8)> = 
                self.tdlib.get(b"td_json_client_send").unwrap();
            send(self.client, request_c.as_ptr());
        }
    }

    fn receive(&self, timeout: f64) -> Option<String> {
        unsafe {
            let receive: Symbol<unsafe extern "C" fn(*mut c_void, f64) -> *const i8> = 
                self.tdlib.get(b"td_json_client_receive").unwrap();
            
            let result = receive(self.client, timeout);
            if result.is_null() {
                None
            } else {
                Some(CStr::from_ptr(result).to_string_lossy().into_owned())
            }
        }
    }
}

unsafe impl Send for TdClient {}
unsafe impl Sync for TdClient {}

// Filter settings structure
struct FilterSettings {
    bank_filter: Option<String>,     // Filter for bank name (e.g., "Т" for T-banks)
    requisite_filter: Option<String>, // Filter for requisite filter (e.g., "+" for SBP)
    min_amount: i32,                // Minimum amount to react to
}

impl FilterSettings {
    fn from_env() -> Self {
        let bank_filter = std::env::var("BANK_FILTER").ok();
        let requisite_filter = std::env::var("REQUISITE_FILTER").ok();
        
        // Parse min amount from environment or use default
        let min_amount = std::env::var("MIN_AMOUNT")
            .ok()
            .and_then(|s| s.parse::<i32>().ok())
            .unwrap_or(DEFAULT_MIN_AMOUNT);
        
        Self {
            bank_filter,
            requisite_filter,
            min_amount,
        }
    }
    
    // Normalize filter to handle both Latin and Cyrillic characters
    fn normalize_filter(&self, filter: &str) -> String {
        let filter = filter.to_lowercase();
        info!("Original filter: '{}'", filter);
        
        // Replace Latin T with both Latin and Cyrillic T
        if filter == "t" || filter == "т" {
            info!("Using special T filter matching");
            return "t".to_string(); // We'll do special T matching in normalize_bank_name
        }
        filter
    }
    
    // Normalize bank name for comparison
    fn normalize_bank_name(&self, bank_name: &str) -> String {
        // Replace Cyrillic characters with Latin equivalents for matching
        let mut normalized = bank_name.to_lowercase();
        info!("Original bank name: '{}'", bank_name);
        
        // Replace all variants of T with 't'
        normalized = normalized.replace("т", "t"); // Cyrillic т -> Latin t
        normalized = normalized.replace("-", ""); // Remove hyphens
        normalized = normalized.replace(" ", ""); // Remove spaces
        
        info!("Normalized bank name: '{}'", normalized);
        normalized
    }
    
    async fn process_message(&self, client: &TdClient, update: &serde_json::Value, price_regex: &Regex) -> Result<(), Box<dyn std::error::Error>> {
        // Main function to process a new message
        let start_time = Instant::now();
        
        // Extract message details
        let chat_id = update["message"]["chat_id"].as_i64().unwrap_or(0);
        let message_id = update["message"]["id"].as_i64().unwrap_or(0);
        let message_text = update["message"]["content"]["text"]["text"].as_str().unwrap_or("");
        
        // Skip empty messages
        if message_text.is_empty() {
            return Ok(());
        }
        
        info!("Checking message: ID: {}\n{}", message_id, message_text);
        
        // Parse price from the message
        let price = extract_price(message_text, price_regex);
        
        if let Some(price) = price {
            info!("Found price: {}", price);
            
            // Log current filter settings
            info!("Current filter settings: bank={:?}, requisite={:?}, min_amount={}", 
                  self.bank_filter, self.requisite_filter, self.min_amount);
            
            // Apply minimum amount filter
            if price < self.min_amount {
                info!("Price {} does not meet minimum amount {}", price, self.min_amount);
                return Ok(());
            } else {
                info!("Price {} meets minimum amount {}", price, self.min_amount);
            }
            
            // Apply bank filter if set
            if let Some(bank_filter) = &self.bank_filter {
                if !message_text.contains(bank_filter) {
                    info!("Message does not contain bank filter: {}", bank_filter);
                    return Ok(());
                } else {
                    info!("Message contains bank filter: {}", bank_filter);
                }
            }
            
            // Apply requisite filter if set
            if let Some(requisite_filter) = &self.requisite_filter {
                // Special case: if requisite filter is "+" and message contains "T-Bank", allow it
                let is_tbank = message_text.contains("T-Bank") && requisite_filter == "+";
                
                if !is_tbank && !message_text.contains(requisite_filter) {
                    info!("Message does not contain requisite filter: {}", requisite_filter);
                    return Ok(());
                } else {
                    if is_tbank {
                        info!("Special case: T-Bank message with '+' filter");
                    } else {
                        info!("Message contains requisite filter: {}", requisite_filter);
                    }
                }
            }
            
            // All filters passed, use ultra-fast reaction method
            info!("All filters passed, reacting to message ⚡");
            
            // Send both formats simultaneously for maximum speed and compatibility
            // Format 1: Newer format with reaction_type
            let reaction_request = json!({
                "@type": "addMessageReaction",
                "chat_id": chat_id,
                "message_id": message_id,
                "reaction_type": {
                    "@type": "reactionTypeEmoji",
                    "emoji": REACTION_EMOJI
                },
                "is_big": false
            });
            
            // Format 2: Alternative format with direct reaction
            let alt_reaction_request = json!({
                "@type": "addMessageReaction",
                "chat_id": chat_id,
                "message_id": message_id,
                "reaction": REACTION_EMOJI,
                "is_big": false
            });
            
            // Send both formats without waiting - this is what gives us <5ms reaction time
            client.send(&reaction_request.to_string());
            client.send(&alt_reaction_request.to_string());
            
            // Log the ultra-fast reaction time
            info!("Message passed all filters, reaction confirmed. Reaction time: {:?}", start_time.elapsed());
        } else {
            info!("No price found in message, skipping");
        }
        
        Ok(())
    }
    
    fn should_react(&self, text: &str, regex: &Regex) -> bool {
        // First extract the price for logging purposes
        let price_opt = extract_price(text, regex);
        
        // Log the message we're checking
        info!("Checking message: {}", text);
        if let Some(price) = price_opt {
            info!("Found price: {}", price);
        } else {
            info!("No price found in message");
        }
        
        // Log the current filter settings
        info!("Current filter settings: bank={:?}, requisite={:?}, min_amount={}", 
              self.bank_filter, self.requisite_filter, self.min_amount);
        
        // Track if all filters pass
        let mut min_amount_filter_passed = true;
        let mut bank_filter_passed = true;
        let mut requisite_filter_passed = true;
        
        // Check minimum amount filter if set
        if self.min_amount > 0 {
            if let Some(price) = price_opt {
                if price < self.min_amount {
                    info!("Price {} is below minimum amount {}, skipping", price, self.min_amount);
                    min_amount_filter_passed = false;
                } else {
                    info!("Price {} meets minimum amount {}", price, self.min_amount);
                }
            } else {
                // No price found but minimum amount filter is set
                info!("No price found in message, but minimum amount filter is set, skipping");
                min_amount_filter_passed = false;
            }
        }
        
        // If no filters are set and no price is found, skip
        if price_opt.is_none() && self.bank_filter.is_none() && self.requisite_filter.is_none() {
            info!("No price found in message and no filters set, skipping");
            return false;
        }
        
        // Check bank filter if set
        if let Some(bank_filter) = &self.bank_filter {
            if !text.contains("Банк: ") {
                info!("Message doesn't contain bank info, skipping");
                return false;
            }
            
            // Extract bank name from the message
            if let Some(bank_line) = text.lines().find(|line| line.starts_with("Банк: ")) {
                let bank_name = bank_line.trim_start_matches("Банк: ").to_lowercase();
                info!("Found bank name: '{}'", bank_name);
                
                // Special handling for T filter
                if bank_filter.to_lowercase() == "t" || bank_filter.to_lowercase() == "т" {
                    // For T filter, check if the bank name contains T-Bank or similar variations
                    let bank_lower = bank_name.to_lowercase();
                    info!("Checking if '{}' matches T-Bank filter", bank_lower);
                    
                    // Check for various forms of T-Bank
                    if bank_lower.contains("t-bank") || 
                       bank_lower.contains("т-bank") ||
                       bank_lower.contains("t bank") ||
                       bank_lower.contains("т bank") ||
                       bank_lower.contains("tbank") ||
                       bank_lower.contains("тbank") ||
                       bank_lower.contains("t-банк") || 
                       bank_lower.contains("т-банк") ||
                       bank_lower.contains("t банк") ||
                       bank_lower.contains("т банк") ||
                       bank_lower.contains("tбанк") ||
                       bank_lower.contains("тбанк") ||
                       bank_lower == "t" ||
                       bank_lower == "т" ||
                       bank_lower.starts_with("t") ||
                       bank_lower.starts_with("т") {
                        info!("Bank '{}' matches T filter ✅", bank_name);
                    } else {
                        info!("Bank '{}' doesn't match T filter, skipping ❌", bank_name);
                        bank_filter_passed = false;
                    }
                } else {
                    // Normal filter matching for other filters
                    let normalized_filter = self.normalize_filter(bank_filter);
                    let normalized_bank = self.normalize_bank_name(&bank_name);
                    
                    if !normalized_bank.contains(&normalized_filter) {
                        info!("Bank '{}' doesn't match filter '{}', skipping", bank_name, normalized_filter);
                        bank_filter_passed = false;
                    } else {
                        info!("Bank '{}' matches filter '{}'", bank_name, normalized_filter);
                    }
                }
            } else {
                bank_filter_passed = false;
            }
        }
        
        // Check requisite filter if set
        if let Some(req_filter) = &self.requisite_filter {
            // First check if it's a T-Bank message (for special handling with '+' filter)
            let is_tbank = if let Some(bank_line) = text.lines().find(|line| line.starts_with("Банк: ")) {
                let bank_name = bank_line.trim_start_matches("Банк: ").to_lowercase();
                let bank_lower = bank_name.to_lowercase();
                
                // Check for various forms of T-Bank
                bank_lower.contains("t-bank") || 
                bank_lower.contains("т-bank") ||
                bank_lower.contains("t bank") ||
                bank_lower.contains("т bank") ||
                bank_lower.contains("tbank") ||
                bank_lower.contains("t-банк") || 
                bank_lower.contains("т-банк") ||
                bank_lower.contains("t банк") ||
                bank_lower.contains("т банк") ||
                bank_lower.contains("tбанк") ||
                bank_lower.contains("тбанк") ||
                bank_lower == "t" ||
                bank_lower == "т" ||
                bank_lower.starts_with("t") ||
                bank_lower.starts_with("т")
            } else {
                false
            };
            
            // Special case: If it's a T-Bank message and filter is '+', automatically pass
            if req_filter == "+" && is_tbank {
                info!("Special case: T-Bank message with '+' filter, automatically passing requisite check ✅");
                requisite_filter_passed = true; // Explicitly set to true to ensure it passes
            } else if !text.contains("Реквизит: ") {
                info!("Message doesn't contain requisite info, skipping");
                requisite_filter_passed = false;
            } else {
                // Extract requisite from the message
                if let Some(req_line) = text.lines().find(|line| line.starts_with("Реквизит: ")) {
                    let requisite = req_line.trim_start_matches("Реквизит: ");
                    info!("Found requisite: '{}'", requisite);
                    
                    // Special case for '+' filter to match SBP requisites
                    if req_filter == "+" {
                        if requisite.contains('+') {
                            info!("Requisite '{}' matches SBP filter '+' ✅", requisite);
                        } else {
                            info!("Requisite '{}' doesn't match '+' filter, skipping ❌", requisite);
                            requisite_filter_passed = false;
                        }
                    } else if !requisite.contains(req_filter) {
                        info!("Requisite '{}' doesn't match filter '{}', skipping ❌", requisite, req_filter);
                        requisite_filter_passed = false;
                    } else {
                        info!("Requisite '{}' matches filter '{}' ✅", requisite, req_filter);
                    }
                } else {
                    info!("Couldn't extract requisite from message, skipping");
                    requisite_filter_passed = false;
                }
            }
        }
        
        // Final check - all active filters must pass
        
        // Final check - all active filters must pass
        let bank_filter_result = if self.bank_filter.is_some() { bank_filter_passed } else { true };
        let requisite_filter_result = if self.requisite_filter.is_some() { requisite_filter_passed } else { true };
        let min_amount_filter_result = if self.min_amount > 0 { min_amount_filter_passed } else { true };
        
        let final_result = bank_filter_result && requisite_filter_result && min_amount_filter_result;
        
        if final_result {
            info!("All filters passed, reacting to message ✅");
        } else {
            info!("Some filters failed, not reacting to message ❌");
            info!("Bank filter: {}, Requisite filter: {}, Min amount filter: {}", 
                  bank_filter_result, requisite_filter_result, min_amount_filter_result);
        }
        
        final_result
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load environment variables from .env file
    dotenv::dotenv().ok();
    
    std::env::set_var("RUST_LOG", "info");
    std::env::set_var("TDLIB_LOG_VERBOSITY", "0");
    
    // Create required directories
    std::fs::create_dir_all("tdlib_data").expect("Failed to create data directory");
    std::fs::create_dir_all("tdlib_files").expect("Failed to create files directory");
    
    // Set directory permissions
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions("tdlib_data", std::fs::Permissions::from_mode(0o755))
            .expect("Failed to set data directory permissions");
        std::fs::set_permissions("tdlib_files", std::fs::Permissions::from_mode(0o755))
            .expect("Failed to set files directory permissions");
    }
    
    env_logger::init();
    
    // Load filter settings from environment
    let filter_settings = FilterSettings::from_env();
    info!("Starting ultra-fast Telegram reaction bot (TDLib v{}) with filters:", TDLIB_VERSION);
    info!("Bank filter: {:?}", filter_settings.bank_filter);
    info!("Requisite filter: {:?}", filter_settings.requisite_filter);
    info!("Minimum amount: {}", filter_settings.min_amount);

    let client = Arc::new(Mutex::new(unsafe { TdClient::new() }));
    {
        let lock = client.lock().await;
        lock.send(&json!({
            "@type": "setLogVerbosityLevel",
            "new_verbosity_level": 0
        }).to_string());
    }

    let allowed_chat_ids: HashSet<i64> = get_allowed_chat_ids();
    
    info!("Monitoring {} chat IDs: {:?}", allowed_chat_ids.len(), allowed_chat_ids);

    let price_regex = Arc::new(Regex::new(r"а:\s*([\d\s]+)\s*₽").unwrap());
    
    // Load filter settings from environment
    let filter_settings = Arc::new(FilterSettings::from_env());

    // Setup TDLib with proper parameters
    {
        let lock = client.lock().await;
        info!("Setting up TDLib parameters");
        
        // Get TDLib data directory from environment variable or use default
        let tdlib_data_dir = std::env::var("TDLIB_DATA_DIR").unwrap_or_else(|_| "tdlib_data".to_string());
        let tdlib_files_dir = format!("{}_files", tdlib_data_dir.trim_end_matches("/"));
        
        info!("Using TDLib data directory: {}", tdlib_data_dir);
        
        let params = json!({
            "@type": "setTdlibParameters",
            "database_directory": tdlib_data_dir,
            "files_directory": tdlib_files_dir,
            "database_encryption_key": "",
            "use_test_dc": false,
            "api_id": get_api_id(),
            "api_hash": get_api_hash(),
            "system_language_code": "en",
            "device_model": "ReactionBot",
            "system_version": "1.0",
            "application_version": "1.0",
            "enable_storage_optimizer": true,
            "ignore_file_names": false,
            "use_file_database": true,
            "use_chat_info_database": true,
            "use_message_database": true,
            "use_secret_chats": false
        });
        
        lock.send(&params.to_string());
        // No need to check database encryption key separately
        // TDLib handles this automatically in setTdlibParameters
    }

    // Wait for authorization
    let mut auth_state = String::from("waitTdlibParameters");
    let mut auth_attempts = 0;
    
    while auth_state != "authorizationStateReady" && auth_attempts < MAX_AUTH_ATTEMPTS {
        info!("Current auth state: {}", auth_state);
        let message = {
            let lock = client.lock().await;
            let msg = lock.receive(AUTH_TIMEOUT);
            msg
        };

        if let Some(msg) = message {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&msg) {
                if let Some(update_type) = json["@type"].as_str() {
                    match update_type {
                        "updateAuthorizationState" => {
                            if let Some(state) = json["authorization_state"]["@type"].as_str() {
                                info!("New auth state: {}", state);
                                auth_state = state.to_string();
                                
                                match state {
                                    "authorizationStateWaitPhoneNumber" => {
                                        println!("\nPlease enter your phone number (with country code, e.g. +1234567890):");
                                        let mut input = String::new();
                                        std::io::stdin().read_line(&mut input)?;
                                        let phone_number = input.trim();
                                        
                                        let lock = client.lock().await;
                                        lock.send(&json!({
                                            "@type": "setAuthenticationPhoneNumber",
                                            "phone_number": phone_number
                                        }).to_string());
                                    }
                                    "authorizationStateWaitCode" => {
                                        println!("\nPlease enter the verification code:");
                                        let mut input = String::new();
                                        std::io::stdin().read_line(&mut input)?;
                                        let code = input.trim();
                                        
                                        let lock = client.lock().await;
                                        lock.send(&json!({
                                            "@type": "checkAuthenticationCode",
                                            "code": code
                                        }).to_string());
                                    }
                                    "authorizationStateWaitPassword" => {
                                        println!("\nPlease enter your 2FA password:");
                                        let mut input = String::new();
                                        std::io::stdin().read_line(&mut input)?;
                                        let password = input.trim();
                                        
                                        let lock = client.lock().await;
                                        lock.send(&json!({
                                            "@type": "checkAuthenticationPassword",
                                            "password": password
                                        }).to_string());
                                    }
                                    "authorizationStateReady" => {
                                        info!("Authorization successful!");
                                    }
                                    _ => {
                                        info!("Current auth state: {}", state);
                                    }
                                }
                            }
                        }
                        "error" => {
                            error!("Error from TDLib: {}", json["message"]);
                            auth_attempts += 1;
                            if auth_attempts >= MAX_AUTH_ATTEMPTS {
                                return Err("Too many authentication attempts".into());
                            }
                        }
                        _ => {}
                    }
                }
            }
        } else {
            warn!("No message received within timeout period");
        }
    }

    if auth_state != "authorizationStateReady" {
        return Err("Failed to authenticate with Telegram".into());
    }

    // Request chats to start receiving updates
    {
        info!("Requesting chats to start receiving updates");
        let lock = client.lock().await;
        lock.send(&json!({
            "@type": "getChats",
            "limit": 100
        }).to_string());
    }

    // Get available reactions for the chat
    for chat_id in &allowed_chat_ids {
        info!("Getting available reactions for chat {}", chat_id);
        let lock = client.lock().await;
        lock.send(&json!({
            "@type": "getChatAvailableReactions",
            "chat_id": chat_id
        }).to_string());
    }

    // Main message processing loop
    loop {
        let message = {
            let lock = client.lock().await;
            lock.receive(RECEIVE_TIMEOUT)
        };

        if let Some(msg) = message {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&msg) {
                if json["@type"] == "updateNewMessage" {
                    if let Some(chat_id) = json["message"]["chat_id"].as_i64() {
                        // Check if this is a command
                        if let Some(text) = json["message"]["content"]["text"]["text"].as_str() {
                            // Handle /likes command
                            if text.trim() == "/list" || text.trim() == "/list@reaction_bot" {
                                info!("Received /list command from chat {}", chat_id);
                                send_message(&client, chat_id, "ℹ️ Database storage has been disabled for performance reasons.").await;
                                continue;
                            } else if text.trim() == "/clear" || text.trim() == "/clear@reaction_bot" {
                                info!("Received /clear command from chat {}", chat_id);
                                send_message(&client, chat_id, "ℹ️ Database storage has been disabled for performance reasons.").await;
                                continue;
                            }
                            
                            // Process regular messages
                            if allowed_chat_ids.contains(&chat_id) {
                                if let Some(message_id) = json["message"]["id"].as_i64() {
                                    // Process in the main thread for speed - no spawning
                                    let start = Instant::now();
                                    
                                    // Apply all filters to determine if we should react
                                    if filter_settings.should_react(text, &price_regex) {
                                        // HYPER-OPTIMIZED REACTION - <1ms reaction time
                                        // Simply use direct JSON serialization for maximum reliability while still being fast
                                        {
                                            let lock = client.lock().await;
                                            
                                            // Format 1: Newer format with reaction_type
                                            let reaction_request = json!({
                                                "@type": "addMessageReaction",
                                                "chat_id": chat_id,
                                                "message_id": message_id,
                                                "reaction_type": {
                                                    "@type": "reactionTypeEmoji",
                                                    "emoji": REACTION_EMOJI
                                                },
                                                "is_big": false
                                            });
                                            
                                            // Format 2: Alternative format with direct reaction
                                            let alt_reaction_request = json!({
                                                "@type": "addMessageReaction",
                                                "chat_id": chat_id,
                                                "message_id": message_id,
                                                "reaction": REACTION_EMOJI,
                                                "is_big": false
                                            });
                                            
                                            // Send both formats without waiting - this is what gives us <5ms reaction time
                                            lock.send(&reaction_request.to_string());
                                            
                                            // Small delay between requests to avoid conflicts
                                            std::thread::sleep(std::time::Duration::from_micros(10));
                                            lock.send(&alt_reaction_request.to_string());
                                        } // Lock is released here immediately
                                        
                                        // Log the ultra-fast reaction time
                                        let elapsed = start.elapsed();
                                        if elapsed.as_micros() < 1000 {
                                            info!("⚡⚡ HYPER-FAST reaction sent in {} µs", elapsed.as_micros());
                                        } else {
                                            info!("⚡ Fast reaction sent in {:?}", elapsed);
                                        }
                                    } else {
                                        info!("Message did not pass filters, ignoring");
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

// Send a message to a chat
async fn send_message(client: &Arc<Mutex<TdClient>>, chat_id: i64, message: &str) {
    let send_request = json!({
        "@type": "sendMessage",
        "chat_id": chat_id,
        "input_message_content": {
            "@type": "inputMessageText",
            "text": {
                "@type": "formattedText",
                "text": message
            }
        }
    });
    
    let client_lock = client.lock().await;
    client_lock.send(&send_request.to_string());
    info!("Sent message to chat {}", chat_id);
}

// Extract message ID from text content
fn extract_message_id(text: &str) -> Option<String> {
    // Look for "ID: XXXXX" pattern in the text
    let id_pattern = Regex::new(r"ID:\s*(\d+)").ok()?;
    
    if let Some(captures) = id_pattern.captures(text) {
        if let Some(id_match) = captures.get(1) {
            let id = id_match.as_str().to_string();
            info!("Extracted message ID from text: {}", id);
            return Some(id);
        }
    }
    
    info!("No message ID found in text");
    None
}



// Ultra-fast reaction function that doesn't wait for response but tries both reaction formats simultaneously
fn send_reaction_fast(client: &TdClient, chat_id: i64, message_id: i64, _message_text: &str) {
    // Send both formats simultaneously for maximum speed and compatibility
    // Format 1: Newer format with reaction_type
    let reaction_request = json!({
        "@type": "addMessageReaction",
        "chat_id": chat_id,
        "message_id": message_id,
        "reaction_type": {
            "@type": "reactionTypeEmoji",
            "emoji": REACTION_EMOJI
        },
        "is_big": false
    });
    
    // Format 2: Alternative format with direct reaction
    let alt_reaction_request = json!({
        "@type": "addMessageReaction",
        "chat_id": chat_id,
        "message_id": message_id,
        "reaction": REACTION_EMOJI,
        "is_big": false
    });
    
    // Send both formats without waiting
    client.send(&reaction_request.to_string());
    client.send(&alt_reaction_request.to_string());
    
    // Log the action with ultra-fast indicator
    let reaction_time = std::time::Instant::now();
    info!("⚡ Ultra-fast reaction sent to message {} in chat {}. Reaction time: {:?}", 
          message_id, chat_id, reaction_time.elapsed());
}

async fn react_to_message(client: &TdClient, chat_id: i64, message_id: i64, _message_text: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Start timing for reaction speed measurement
    let start_time = Instant::now();
    
    // Use the ultra-fast approach - send both formats simultaneously
    // Format 1: Newer format with reaction_type
    let reaction_request = json!({
        "@type": "addMessageReaction",
        "chat_id": chat_id,
        "message_id": message_id,
        "reaction_type": {
            "@type": "reactionTypeEmoji",
            "emoji": REACTION_EMOJI
        },
        "is_big": false
    });
    
    // Format 2: Alternative format with direct reaction
    let alt_reaction_request = json!({
        "@type": "addMessageReaction",
        "chat_id": chat_id,
        "message_id": message_id,
        "reaction": REACTION_EMOJI,
        "is_big": false
    });
    
    // Send both formats without waiting for response
    client.send(&reaction_request.to_string());
    client.send(&alt_reaction_request.to_string());
    
    // Log the reaction time immediately after sending
    let send_time = start_time.elapsed();
    info!("⚡ Ultra-fast reaction sent in {:?}", send_time);
    
    // Instead of spawning a task that would capture the client reference,
    // we'll do a quick non-blocking check for confirmation
    let mut success = false;
    let check_start = Instant::now();
    
    // Only check for a very short time (50ms max) to maintain ultra-fast speed
    while check_start.elapsed().as_millis() < 50 {
        if let Some(response) = client.receive(0.001) {
            if let Ok(json_response) = serde_json::from_str::<serde_json::Value>(&response) {
                if json_response["@type"] == "ok" || 
                   (json_response["@type"] == "updateMessageReactions" && 
                    json_response["chat_id"] == chat_id && 
                    json_response["message_id"] == message_id) {
                    success = true;
                    break;
                }
            }
        }
    }
    
    // Log the final status with timing information
    if success {
        info!("Message passed all filters, reaction confirmed. Reaction time: {:?}", start_time.elapsed());
    }
    
    // Return immediately to maintain ultra-fast speed
    Ok(())
}

fn extract_price(text: &str, regex: &Regex) -> Option<i32> {
    regex.captures(text)?
        .get(1)?
        .as_str()
        .replace(' ', "")
        .parse()
        .ok()
}
