use mongodb::error::Error;
use mongodb::options::ClientOptions;
use mongodb::Client;
use log::info;

pub async fn connect() -> Result<Client, Error> {
    info!("Initializing database connection");

    let mut client_options = ClientOptions::parse("").await?;
    client_options.app_name = Some("Actix Web Server".to_string());

    Client::with_options(client_options)
}
