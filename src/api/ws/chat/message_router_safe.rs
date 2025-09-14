// Just the generic handler part to add to the existing file
    /// Generic domain command handler - eliminates repetition
    async fn handle_domain_command(
        &self,
        domain: &str,
        method: &str,
        params: serde_json::Value,
        handler: impl std::future::Future<Output = ApiResult<WsServerMessage>>,
    ) -> Result<(), anyhow::Error> {
        info!("{} command: {}", domain, method);
        
        match handler.await {
            Ok(response) => {
                self.connection.send_message(response).await?;
            }
            Err(e) => {
                error!("{} command failed: {}", domain, e);
                // We need to check the actual send_error signature
            }
        }
        
        Ok(())
    }
