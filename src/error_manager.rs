use tokio::sync::broadcast::Receiver;
use crate::utils::shutdown::Shutdown;

pub struct ErrorManager {
    pub mq_url: String,
    shutdown: Shutdown
}

impl ErrorManager {
    pub fn new(mq_url: String, shutdown_receiver: Receiver<()>) -> Self {
        Self {
            mq_url,
            shutdown: Shutdown::new(shutdown_receiver)
        }
    }

    pub async fn run(&mut self) {
        
    }
}