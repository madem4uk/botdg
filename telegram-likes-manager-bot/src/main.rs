use std::{process::{Child, Command as ProcessCommand}, sync::Arc, env};
use tokio::sync::Mutex;
use log::info;
use teloxide::prelude::*;
use teloxide::utils::command::BotCommands;
use dotenv::dotenv;
use anyhow::Result;

// Global state to track the reaction bot process
struct BotState {
    reaction_bot_process: Option<Child>,
    is_running: bool,
    last_status: String,
    bank_filter: Option<String>,
    requisite_filter: Option<String>,
    min_amount: i32,
}

impl BotState {
    fn new() -> Self {
        Self {
            reaction_bot_process: None,
            is_running: false,
            last_status: "Not started".to_string(),
            bank_filter: None,
            requisite_filter: None,
            min_amount: 38000, // Default minimum amount
        }
    }
}

// Define bot commands
#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase", description = "These commands are supported:")]
enum TelegramCommand {
    #[command(description = "Start the reaction bot")]
    Start,
    
    #[command(description = "Stop the reaction bot")]
    Stop,
    
    #[command(description = "Check if the reaction bot is running")]
    Status,
    
    #[command(description = "Set the bank filter (e.g., /bank t for T-Bank)")]
    Bank { filter: String },
    
    #[command(description = "Set the requisite filter (e.g., /requisite + for SBP)")]
    Requisite { filter: String },
    
    #[command(description = "Set the minimum amount (e.g., /amount 50000)")]
    Amount { value: i32 },
    
    #[command(description = "Clear all filters")]
    Clear,
    
    #[command(description = "Display this help message")]
    Help,
}

async fn handle_command(
    bot: Bot,
    message: Message,
    command: TelegramCommand,
    bot_state: Arc<Mutex<BotState>>,
) -> Result<()> {
    let chat_id = message.chat.id;
    
    match command {
        TelegramCommand::Start => {
            let mut state = bot_state.lock().await;
            
            if state.is_running {
                bot.send_message(chat_id, "The reaction bot is already running.").await?;
                return Ok(());
            }
            
            // Get reaction bot path from environment
            let reaction_bot_path = env::var("REACTION_BOT_PATH")
                .unwrap_or_else(|_| "/Users/h/Rustown/telegram-reaction-bot".to_string());
            
            // First, make sure no existing instances are running
            if cfg!(target_os = "windows") {
                let _ = ProcessCommand::new("taskkill")
                    .args(["/F", "/IM", "tdlib-test.exe"])
                    .output();
            } else {
                let _ = ProcessCommand::new("pkill")
                    .args(["-f", "tdlib-test"])
                    .output();
            }
            
            // For maximum speed, use the pre-built binary directly instead of cargo run
            // This significantly reduces startup time and improves reaction speed
            let binary_path = format!("{}/target/release/tdlib-test", reaction_bot_path);
            
            // Check if the binary exists, if not, build it first
            if !std::path::Path::new(&binary_path).exists() {
                // Build the reaction bot first
                bot.send_message(chat_id, "🔨 Building reaction bot (one-time setup)...").await?;
                
                let build_result = ProcessCommand::new("cargo")
                    .current_dir(&reaction_bot_path)
                    .arg("build")
                    .arg("--release")
                    .output();
                
                if let Err(e) = build_result {
                    state.last_status = format!("Failed to build: {}", e);
                    bot.send_message(
                        chat_id, 
                        format!("❌ Failed to build reaction bot: {}", e)
                    ).await?;
                    return Ok(());
                }
            }
            
            // Set environment variables for the reaction bot based on filters
            let mut command = ProcessCommand::new(&binary_path);
            
            // Set bank filter if specified
            if let Some(bank) = &state.bank_filter {
                command.env("BANK_FILTER", bank);
            }
            
            // Set requisite filter if specified
            if let Some(requisite) = &state.requisite_filter {
                command.env("REQUISITE_FILTER", requisite);
            }
            
            // Set minimum amount
            command.env("MIN_AMOUNT", state.min_amount.to_string());
            
            // Special handling for T-Bank messages when requisite filter is set to "+"
            // This ensures T-Bank messages are included even if they don't have a "+" in their requisite
            if state.requisite_filter.as_deref() == Some("+") {
                info!("Special handling for T-Bank messages with '+' filter is enabled");
            }
            
            match command.spawn() {
                Ok(child) => {
                    state.reaction_bot_process = Some(child);
                    state.is_running = true;
                    state.last_status = "Running".to_string();
                    
                    let filter_info = format!(
                        "Bank filter: {}\nRequisite filter: {}\nMinimum amount: {}",
                        state.bank_filter.as_deref().unwrap_or("None"),
                        state.requisite_filter.as_deref().unwrap_or("None"),
                        state.min_amount
                    );
                    
                    bot.send_message(
                        chat_id, 
                        format!("✅ Reaction bot started successfully with the following settings:\n\n{}", filter_info)
                    ).await?;
                },
                Err(e) => {
                    state.last_status = format!("Failed to start: {}", e);
                    bot.send_message(
                        chat_id, 
                        format!("❌ Failed to start reaction bot: {}", e)
                    ).await?;
                }
            }
        },
        
        TelegramCommand::Stop => {
            let mut state = bot_state.lock().await;
            
            if !state.is_running {
                bot.send_message(chat_id, "The reaction bot is not running.").await?;
                return Ok(());
            }
            
            // Print to terminal that we're stopping the bot
            println!("\n==== STOPPING REACTION BOT ====\n");
            
            // More reliable process termination using system commands
            if let Some(mut child) = state.reaction_bot_process.take() {
                // First try graceful termination
                let pid = child.id();
                info!("Attempting to stop reaction bot process with PID {}", pid);
                println!("Stopping reaction bot process with PID {}", pid);
                
                // Use kill command to terminate the process and its children
                let kill_command = if cfg!(target_os = "windows") {
                    format!("taskkill /F /T /PID {}", pid)
                } else {
                    format!("pkill -TERM -P {}", pid)
                };
                
                println!("Executing: {}", kill_command);
                
                let kill_result = if cfg!(target_os = "windows") {
                    ProcessCommand::new("taskkill")
                        .args(["/F", "/T", "/PID", &pid.to_string()])
                        .output()
                } else {
                    // On Unix systems, use pkill to kill the process group
                    ProcessCommand::new("pkill")
                        .args(["-TERM", "-P", &pid.to_string()])
                        .output()
                };
                
                match kill_result {
                    Ok(output) => {
                        // Print the command output to terminal
                        if !output.stdout.is_empty() {
                            println!("Command output: {}", String::from_utf8_lossy(&output.stdout));
                        }
                        if !output.stderr.is_empty() {
                            println!("Command error: {}", String::from_utf8_lossy(&output.stderr));
                        }
                        
                        // Also try to kill the process directly
                        println!("Also killing process directly");
                        let _ = child.kill();
                        state.is_running = false;
                        state.last_status = "Stopped".to_string();
                        bot.send_message(chat_id, "✅ Reaction bot stopped successfully.").await?;
                        println!("✅ Reaction bot stopped successfully.");
                    },
                    Err(e) => {
                        println!("Error with kill command: {}", e);
                        // Try direct kill as fallback
                        println!("Trying direct kill as fallback");
                        match child.kill() {
                            Ok(_) => {
                                state.is_running = false;
                                state.last_status = "Stopped".to_string();
                                bot.send_message(chat_id, "✅ Reaction bot stopped successfully (fallback method).").await?;
                                println!("✅ Reaction bot stopped successfully (fallback method).");
                            },
                            Err(e2) => {
                                println!("Failed to kill process: {}", e2);
                                state.reaction_bot_process = Some(child);
                                bot.send_message(
                                    chat_id, 
                                    format!("❌ Failed to stop reaction bot: {} (fallback error: {})", e, e2)
                                ).await?;
                            }
                        }
                    }
                }
            } else {
                // No child process found, but state says it's running
                println!("No child process found, but state says it's running");
                println!("Killing any potential orphaned processes");
                
                // Kill any potential orphaned processes
                let kill_command = if cfg!(target_os = "windows") {
                    "taskkill /F /IM tdlib-test.exe"
                } else {
                    "pkill -f tdlib-test"
                };
                
                println!("Executing: {}", kill_command);
                
                let output = if cfg!(target_os = "windows") {
                    ProcessCommand::new("taskkill")
                        .args(["/F", "/IM", "tdlib-test.exe"])
                        .output()
                } else {
                    ProcessCommand::new("pkill")
                        .args(["-f", "tdlib-test"])
                        .output()
                };
                
                if let Ok(output) = output {
                    // Print the command output to terminal
                    if !output.stdout.is_empty() {
                        println!("Command output: {}", String::from_utf8_lossy(&output.stdout));
                    }
                    if !output.stderr.is_empty() {
                        println!("Command error: {}", String::from_utf8_lossy(&output.stderr));
                    }
                }
                
                state.is_running = false;
                state.last_status = "Stopped".to_string();
                bot.send_message(chat_id, "✅ Reaction bot stopped successfully.").await?;
                println!("✅ Reaction bot stopped successfully.");
            }
            
            println!("\n==== REACTION BOT STOPPED ====\n");
        },
        
        TelegramCommand::Status => {
            let state = bot_state.lock().await;
            
            let status = if state.is_running {
                "✅ Running"
            } else {
                "❌ Not running"
            };
            
            let filter_info = format!(
                "Bank filter: {}\nRequisite filter: {}\nMinimum amount: {}",
                state.bank_filter.as_deref().unwrap_or("None"),
                state.requisite_filter.as_deref().unwrap_or("None"),
                state.min_amount
            );
            
            bot.send_message(
                chat_id, 
                format!("Reaction bot status: {}\n\nCurrent settings:\n{}", status, filter_info)
            ).await?;
        },
        
        TelegramCommand::Bank { filter } => {
            let mut state = bot_state.lock().await;
            
            if filter.trim().to_lowercase() == "none" || filter.trim().is_empty() {
                state.bank_filter = None;
                bot.send_message(chat_id, "✅ Bank filter cleared.").await?;
            } else {
                state.bank_filter = Some(filter.clone());
                bot.send_message(chat_id, format!("✅ Bank filter set to: {}", filter)).await?;
            }
            
            // If the bot is running, we need to restart it for the changes to take effect
            if state.is_running {
                bot.send_message(
                    chat_id, 
                    "⚠️ Please restart the bot with /stop and then /start for the changes to take effect."
                ).await?;
            }
        },
        
        TelegramCommand::Requisite { filter } => {
            let mut state = bot_state.lock().await;
            
            if filter.trim().to_lowercase() == "none" || filter.trim().is_empty() {
                state.requisite_filter = None;
                bot.send_message(chat_id, "✅ Requisite filter cleared.").await?;
            } else {
                state.requisite_filter = Some(filter.clone());
                
                // Special note for "+" filter about T-Bank handling
                if filter == "+" {
                    bot.send_message(
                        chat_id, 
                        format!("✅ Requisite filter set to: {}\n\n⚠️ Note: With '+' filter, the bot will also react to T-Bank messages regardless of their requisite.", filter)
                    ).await?;
                } else {
                    bot.send_message(chat_id, format!("✅ Requisite filter set to: {}", filter)).await?;
                }
            }
            
            // If the bot is running, we need to restart it for the changes to take effect
            if state.is_running {
                bot.send_message(
                    chat_id, 
                    "⚠️ Please restart the bot with /stop and then /start for the changes to take effect."
                ).await?;
            }
        },
        
        TelegramCommand::Amount { value } => {
            let mut state = bot_state.lock().await;
            
            state.min_amount = value;
            bot.send_message(chat_id, format!("✅ Minimum amount set to: {}", value)).await?;
            
            // If the bot is running, we need to restart it for the changes to take effect
            if state.is_running {
                bot.send_message(
                    chat_id, 
                    "⚠️ Please restart the bot with /stop and then /start for the changes to take effect."
                ).await?;
            }
        },
        
        TelegramCommand::Clear => {
            let mut state = bot_state.lock().await;
            
            state.bank_filter = None;
            state.requisite_filter = None;
            state.min_amount = 38000; // Reset to default
            
            bot.send_message(chat_id, "✅ All filters cleared and minimum amount reset to default (38000).").await?;
            
            // If the bot is running, we need to restart it for the changes to take effect
            if state.is_running {
                bot.send_message(
                    chat_id, 
                    "⚠️ Please restart the bot with /stop and then /start for the changes to take effect."
                ).await?;
            }
        },
        
        TelegramCommand::Help => {
            bot.send_message(
                chat_id,
                TelegramCommand::descriptions().to_string(),
            ).await?;
        }
    }
    
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize environment variables and logging
    dotenv().ok();
    pretty_env_logger::init();
    
    let bot_token = env::var("BOT_TOKEN").expect("BOT_TOKEN must be set in .env file");
    let allowed_users = env::var("ALLOWED_USERS")
        .unwrap_or_default()
        .split(',')
        .filter_map(|s| s.trim().parse::<i64>().ok())
        .collect::<Vec<i64>>();
    
    info!("Starting Telegram controller bot");
    info!("Allowed users: {:?}", allowed_users);
    
    // Create bot state
    let bot_state = Arc::new(Mutex::new(BotState::new()));
    
    // Create bot instance
    let bot = Bot::new(bot_token);
    
    // Set bot commands
    bot.set_my_commands(TelegramCommand::bot_commands()).await?;
    
    // Clone allowed_users for the closure
    let allowed_users_clone = allowed_users.clone();
    
    // Start command handler
    let handler = Update::filter_message()
        .filter_map(move |message: Message| {
            let user_id = message.from().map(|user| user.id.0 as i64);
            
            // Check if user is allowed
            if let Some(user_id) = user_id {
                if !allowed_users_clone.is_empty() && !allowed_users_clone.contains(&user_id) {
                    info!("Unauthorized access attempt from user {}", user_id);
                    return None;
                }
            } else {
                return None;
            }
            
            Some(message)
        })
        .branch(
            dptree::entry()
                .filter_command::<TelegramCommand>()
                .endpoint(handle_command),
        );
    
    // Start the bot
    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![bot_state])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;
    
    Ok(())
}
