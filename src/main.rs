use std::time::Duration;
use dotenv::dotenv;
use tokio::runtime::Builder;
use tracing_subscriber::fmt::format;

#[derive(Clone)]
pub struct AppState {
    amqp_url: String,
}

fn main() {
    // Simple .env existence check
    dotenv().ok();
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
        }
    };

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

    });
}
