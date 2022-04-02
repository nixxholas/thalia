#[macro_use]
extern crate tokio;

mod log_manager;
mod utils;

use std::time::Duration;
use dotenv::dotenv;
use tokio::runtime::Builder;
use tokio::select;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::broadcast;
use tracing::{error, info};
use crate::log_manager::LogManager;

#[derive(Clone)]
pub struct AppState {
    amqp_url: String,
    amqp_exchange: String
}

fn main() {
    // Simple .env existence check
    dotenv().ok();
    // Initial the global logger
    tracing_subscriber::fmt::init();
    // Dev? Production?
    let env_type = std::env::var("ENVIRONMENT").expect("ENVIRONMENT must be set").to_uppercase();
    // Retrieve the current machine name just in case we need it for environment setups
    let machine_name = whoami::username().to_uppercase();

    // Setup the app_state that stores most, if not all of the machine env states
    let app_state = AppState {
        amqp_url: match env_type.as_str() {
            "DEVELOPMENT" | "DEV" => std::env::var(format!("{}_AMQP_URL", machine_name))
                .expect(&*format!("{}_AMQP_URL must be set", machine_name)),
            _ => std::env::var("AMQP_URL").expect("AMQP_URL must be set")
        },
        amqp_exchange: std::env::var("MQ_EXCHANGE").expect("MQ_EXCHANGE must be set")
    };

    // When the provided `shutdown` future completes, we must send a shutdown
    // message to all active connections. We use a broadcast channel for this
    // purpose. The call below ignores the receiver of the broadcast pair, and when
    // a receiver is needed, the subscribe() method on the sender is used to create
    // one.
    let (notify_shutdown, _) = broadcast::channel(1);

    // Setup the tokio runtime for running the entire instance.
    let rt = Builder::new_multi_thread()
        .enable_all()
        .worker_threads(num_cpus::get())
        .thread_keep_alive(Duration::from_secs(60000))
        .thread_stack_size(96 * 1024 * 1024)
        .build()
        .unwrap();

    // Start an async instance of thalia.
    rt.block_on(async {
        let mut log_manager = LogManager::new(app_state.amqp_url,
                                              app_state.amqp_exchange,
                                              std::env::var("SLACK_HOOK")
                                                  .expect("SLACK_HOOK must be set"),
                                              notify_shutdown.clone());

        let application_thread = tokio::spawn(async move {
            log_manager.run().await;
        });

        let mut alarm_sig = signal(SignalKind::alarm()).expect("Alarm stream failed.");
        let mut hangup_sig = signal(SignalKind::hangup()).expect("Hangup stream failed.");
        let mut int_sig = signal(SignalKind::interrupt()).expect("Interrupt stream failed.");
        let mut pipe_sig = signal(SignalKind::pipe()).expect("Pipe stream failed.");
        let mut quit_sig = signal(SignalKind::quit()).expect("Quit stream failed.");
        let mut term_sig = signal(SignalKind::terminate()).expect("Terminate stream failed.");
        let mut ud1_sig = signal(SignalKind::user_defined1()).expect("UD1 signal stream failed.");
        let mut ud2_sig = signal(SignalKind::user_defined2()).expect("UD2 signal stream failed.");
        select! {
            _ = alarm_sig.recv() => {
                info!("SIGALRM received, terminating the indexer now!");
            }
            _ = hangup_sig.recv() => {
                info!("SIGHUP received, terminating the indexer now!");
            }
            _ = int_sig.recv() => {
                info!("SIGINT received, terminating the indexer now!");
            }
            _ = pipe_sig.recv() => {
                info!("SIGPIPE received, terminating the indexer now!");
            }
            _ = quit_sig.recv() => {
                info!("SIGQUIT received, terminating the indexer now!");
            }
            _ = term_sig.recv() => {
                info!("SIGTERM received, terminating the indexer now!");
            }
            _ = ud1_sig.recv() => {
                info!("SIGUSR1 received, terminating tdhe indexer now!");
            }
            _ = ud2_sig.recv() => {
                info!("SIGUSR2 received, terminating the indexer now!");
            }
        }

        drop(notify_shutdown);

        if let Err(err) = application_thread.await {
            error!(
                "There was an error terminating the application gracefully due to {}",
                err
            );
        } else {
            info!("Indexer was gracefully terminated.");
        }
    });
}
