use std::{
    error::Error,
    time::{Duration, Instant},
};
use tonic::transport::{Channel, Endpoint};

mod saleae {
    tonic::include_proto!("saleae.automation");
}

// Type-check a real generated RPC without requiring a running server.
#[allow(dead_code)]
async fn app_info(
    client: &mut saleae::manager_client::ManagerClient<Channel>,
) -> Result<saleae::GetAppInfoReply, tonic::Status> {
    Ok(client
        .get_app_info(saleae::GetAppInfoRequest {})
        .await?
        .into_inner())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let started = Instant::now();
    let endpoint = Endpoint::from_static("http://localhost:10430")
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(2));
    let result = tokio::time::timeout(Duration::from_secs(3), endpoint.connect()).await;
    match result {
        Ok(Err(error)) => {
            let mut chain = vec![error.to_string()];
            let mut source = error.source();
            while let Some(cause) = source {
                chain.push(cause.to_string());
                source = cause.source();
            }
            // Reject unrelated DNS/configuration failures as a successful no-app test.
            if !chain.iter().any(|s| s.to_lowercase().contains("refused")) {
                return Err(format!("unexpected connection error: {chain:?}").into());
            }
            println!(
                "{}",
                serde_json::json!({
                    "result": "expected_connection_failure_handled",
                    "endpoint": "localhost:10430", "error_chain": chain,
                    "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
                    "panic": false, "device_or_app_used": false
                })
            );
            Ok(())
        }
        Ok(Ok(channel)) => {
            let _client = saleae::manager_client::ManagerClient::new(channel);
            Err("a server is listening; no-app failure precondition not met".into())
        }
        Err(_) => Err("outer deadline expired instead of connection refused".into()),
    }
}
