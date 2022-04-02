use std::borrow::Borrow;
use amq_protocol_types::FieldTable;
use deadpool_lapin::Runtime;
use futures::{StreamExt};
use futures::future::err;
use lapin::ExchangeKind;
use lapin::message::Delivery;
use lapin::options::{BasicAckOptions, BasicConsumeOptions, BasicQosOptions,
                     ExchangeDeclareOptions, QueueDeclareOptions};
use serde::{
    Deserialize, Serialize
};
use tokio::{
    select,
    sync::broadcast::Sender
};
use tracing::{error, info};

use crate::utils::shutdown::Shutdown;

pub struct LogManager {
    mq_url: String,
    mq_exchange: String,
    error_slack_hook: String,
    shutdown_broadcaster: Sender<()>
}

#[derive(Debug, Serialize)]
struct SlackMessage {
    text: String,
}

impl SlackMessage {
    fn from_error(error: Error) -> Self {
        Self {
            text: serde_json::to_string(&error).unwrap()
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
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

#[derive(Debug, Deserialize, Serialize)]
struct Event {
    // Which microservice are we talking about?
    domain: String,
    // Which function is this error coming from? (Which API for example)
    function: String,
    // What are the intricate details to the crash?
    message: String
}

impl LogManager {
    pub fn new(mq_url: String, mq_exchange: String, error_slack_hook: String,
               shutdown_broadcaster: Sender<()>) -> Self {
        Self {
            mq_url,
            mq_exchange,
            error_slack_hook,
            shutdown_broadcaster
        }
    }

    pub async fn run(&mut self) {
        let (em_mq_url, em_slack_hook, em_mq_exchange, mut em_shutdown) =
            (self.mq_url.clone(),
            self.error_slack_hook.clone(),
             self.mq_exchange.clone(),
             Shutdown::new(self.shutdown_broadcaster.subscribe()));
        let error_manager_thread = tokio::spawn(async move {
            let mut amqp_cfg = deadpool_lapin::Config::default();
            amqp_cfg.url = Some(em_mq_url);
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
                    &em_mq_exchange,
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
                _ = em_shutdown.recv() => break,
                msg = consumer.next() => {
                    // Should there be a delivery, we take it in
                    if let Some(unwrapped_msg) = msg {
                        let delivery: Delivery = unwrapped_msg.expect("Error in consumer!");
                        let routing_key = delivery.routing_key.to_string();
                        let error_result = serde_json::from_slice::<Error>(delivery.data.as_slice());

                        if let Ok(error) = error_result {
                            info!("[{}] New error received: {:?}", routing_key, error);
                            post_to_slack(&em_slack_hook, SlackMessage::from_error(error.clone()))
                                .await;
                        }

                        let _ = delivery.ack(BasicAckOptions { multiple: false }).await;
                    }
                }
            }
            }
        });

        let (log_mq_url, log_mq_exchange, mut log_shutdown) = (self.mq_url.clone(),
                                                               self.mq_exchange.clone(),
                                                               Shutdown::new(
                                                                self.shutdown_broadcaster.subscribe()));
        let log_manager_thread = tokio::spawn(async move {
            let mut amqp_cfg = deadpool_lapin::Config::default();
            amqp_cfg.url = Some(log_mq_url);
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
                    &log_mq_exchange,
                    ExchangeKind::Topic,
                    ExchangeDeclareOptions::default(),
                    FieldTable::default(),
                )
                .await
                .expect("Expected MQ Exchange declaration to succeed.");
            channel
                .queue_declare(
                    "*.logs",
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
                    "*.logs",
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
                _ = log_shutdown.recv() => break,
                msg = consumer.next() => {
                    // Should there be a delivery, we take it in
                    if let Some(unwrapped_msg) = msg {
                        let delivery: Delivery = unwrapped_msg.expect("Error in consumer!");
                        let routing_key = delivery.routing_key.to_string();
                        let error_result = serde_json::from_slice::<Error>(delivery.data.as_slice());

                        if let Ok(error) = error_result {
                            info!("[{}] New activity received: {:?}", routing_key, error);
                        }

                        let _ = delivery.ack(BasicAckOptions { multiple: false }).await;
                    }
                }
            }
            }
        });

        log_manager_thread.await;
        error_manager_thread.await;
    }
}

async fn post_to_slack(hook: &str, message: SlackMessage) {
    let client = reqwest::Client::new();
    let post_result = client.post(hook)
        .body(serde_json::to_string(&message).unwrap())
        .send()
        .await;

    if let Err(post_err) = post_result {
        error!("There was an issue pushing the error item to Slack! Data: {:?}", post_err);
    }
}