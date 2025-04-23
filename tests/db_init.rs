mod common;

#[tokio::test]
async fn test_db_init() -> Result<(), Box<dyn std::error::Error>> {
	common::init_db().await?.health().await?;
	Ok(())
}
