/*
 * This file is part of Astarte.
 *
 * Copyright 2023 SECO Mind Srl
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *    http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 *
 * SPDX-License-Identifier: Apache-2.0
 */

//! Astarte Message Hub client example, will send the uptime every 3 seconds to Astarte.

use astarte_device_sdk::builder::{DeviceBuilder, DeviceSdkBuild};
use astarte_device_sdk::store::memory::MemoryStore;
use astarte_device_sdk::transport::grpc::GrpcConfig;
use astarte_device_sdk::types::AstarteType;
use astarte_device_sdk::{Client, ClientDisconnect};
use std::time;

use clap::Parser;
use log::{error, info, warn};
use tokio::select;
use tokio::signal::ctrl_c;
use tokio::task::JoinHandle;
use uuid::Uuid;

/// Create a ProtoBuf client for the Astarte message hub.
#[derive(Parser, Debug)]
#[clap(version, about)]
struct Cli {
    /// UUID to be used when registering the client as an Astarte message hub node.
    uuid: String,

    /// Endpoint of the Astarte Message Hub server instance
    #[clap(default_value = "http://[::1]:50051")]
    endpoint: String,

    /// Stop after sending COUNT messages.
    #[clap(short, long)]
    count: Option<u64>,

    /// Milliseconds to wait between messages.
    #[clap(short, long, default_value = "3000")]
    time: u64,
}

type DynError = Box<dyn std::error::Error + Send + Sync + 'static>;

#[tokio::main]
async fn main() -> Result<(), DynError> {
    env_logger::init();
    let args = Cli::parse();
    let node_id = Uuid::parse_str(&args.uuid).expect("Node id not specified");

    let grpc_cfg = GrpcConfig::new(node_id, args.endpoint);

    let (mut node, mut rx_events) = DeviceBuilder::new()
        .store(MemoryStore::new())
        .interface_directory("examples/client/interfaces")
        .expect("failed to use interface directory")
        .connect(grpc_cfg)
        .await
        .expect("failed to connect")
        .build();

    let receive_handle = tokio::spawn(async move {
        info!("start receiving messages from the Astarte Message Hub Server");
        while let Some(res) = rx_events.recv().await {
            match res {
                Ok(data) => {
                    if let astarte_device_sdk::Aggregation::Individual(AstarteType::Boolean(val)) =
                        data.data
                    {
                        info!("received {val}");
                    }
                }
                Err(err) => {
                    error!("error while receiving data from Astarte Message Hub Server: {err:?}")
                }
            }
        }
    });

    let node_cpy = node.clone();

    // Create a task to transmit
    let send_handle = tokio::spawn(async move {
        let now = time::SystemTime::now();
        let mut count = 0;
        // Consistent interval of 3 seconds
        let mut interval = tokio::time::interval(time::Duration::from_millis(args.time));

        while args.count.is_none() || Some(count) < args.count {
            interval.tick().await;

            info!("Publishing the uptime through the message hub.");

            let elapsed = now.elapsed().unwrap().as_secs();

            let elapsed_str = format!("Uptime for node {}: {}", args.uuid, elapsed);

            node_cpy
                .send(
                    "org.astarte-platform.rust.examples.datastream.DeviceDatastream",
                    "/uptime",
                    elapsed_str,
                )
                .await
                .expect("failed to send Astarte message");

            count += 1;
        }

        info!("Done sending messages");
    });

    // wait for CTRL C to terminate the node execution
    select! {
        _ = ctrl_c() => {
            send_handle.abort();
            receive_handle.abort();
        }
        res = node.handle_events() => {
            warn!("disconnected from Astarte");
            return res.map_err(Into::into);
        }
    }

    handle_task(receive_handle).await;
    handle_task(send_handle).await;

    node.disconnect().await;

    Ok(())
}

async fn handle_task(h: JoinHandle<()>) {
    match h.await {
        Ok(()) => {}
        Err(err) if err.is_cancelled() => {}
        Err(err) => error!("join error: {err:?}"),
    };
}
