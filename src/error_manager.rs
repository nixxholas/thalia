use amq_protocol_types::FieldTable;
use deadpool_lapin::Runtime;
use futures::{StreamExt};
use futures::future::err;
use lapin::ExchangeKind;
use lapin::message::Delivery;
use lapin::options::{BasicConsumeOptions, BasicQosOptions, ExchangeDeclareOptions, QueueDeclareOptions};
use serde::{
    Deserialize, Serialize
};
use tokio::{
    select,
    sync::broadcast::Receiver
};
use tracing::info;

use crate::utils::shutdown::Shutdown;

pub struct ErrorManager {
    mq_url: String,
    mq_exchange: String,
    shutdown: Shutdown
}

#[derive(Debug, Deserialize, Serialize)]
struct Error {
    // Which microservice are we talking about?
    domain: String,
    // Which function is this error coming from? (Which API for example)
    function: String,
    // Why did it crash?
    reason: String,
    // What are the intricate details to the crash?
    message: String
}

impl ErrorManager {
    pub fn new(mq_url: String, mq_exchange: String, shutdown_receiver: Receiver<()>) -> Self {
        Self {
            mq_url,
            mq_exchange,
            shutdown: Shutdown::new(shutdown_receiver)
        }
    }

    pub async fn run(&mut self) {
        let mut amqp_cfg = deadpool_lapin::Config::default();
        amqp_cfg.url = Some(self.mq_url.to_string());
        let mq_pool = amqp_cfg.create_pool(Some(Runtime::Tokio1))
            .expect("Problem creating an AMQP connection pool!");

        // Get the AMQP connection up and running first.
        let current_connection = mq_pool
            .get()
            .await
            .expect("Expected MQ pool to provide a connection.");
        let channel = current_connection
            .create_channel()
            .await
            .expect("Expected MQ Connection to provide a channel");

        channel
            .exchange_declare(
                self.mq_exchange.as_str(),
                ExchangeKind::Topic,
                ExchangeDeclareOptions::default(),
                FieldTable::default(),
            )
            .await
            .expect("Expected MQ Exchange declaration to succeed.");
        channel
            .queue_declare(
                "*.error",
                QueueDeclareOptions {
                    passive: false,
                    durable: true,
                    exclusive: false,
                    auto_delete: false,
                    nowait: false,
                },
                FieldTable::default(),
            )
            .await
            .expect("Expected MQ Queue declaration to succeed.");
        channel
            .basic_qos(1, BasicQosOptions { global: false })
            .await
            .expect("MQ Channel QOS must be set!");

        let consumer = channel
            .basic_consume(
                "*.error",
                &*format!("thalia_{}", &*whoami::desktop_env().to_string()),
                BasicConsumeOptions {
                    no_local: false,
                    no_ack: false,
                    exclusive: false,
                    nowait: false,
                },
                FieldTable::default(),
            )
            .await
            .expect("Consumer should be actively connected");

        tokio::pin!(consumer);
        loop {
            select! {
                _ = self.shutdown.recv() => break,
                msg = consumer.next() => {
                    // Should there be a delivery, we take it in
                    if let Some(unwrapped_msg) = msg {
                        let delivery: Delivery = unwrapped_msg.expect("Error in consumer!");
                        let routing_key = delivery.routing_key.to_string();
                        let error_result = serde_json::from_slice::<Error>(delivery.data.as_slice());

                        if let Ok(error) = error_result {
                            info!("[{}] New error received: {:?}", routing_key, error);
                        }
                    }
                }
            }
        }
    }
}