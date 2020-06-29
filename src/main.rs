#[macro_use]
extern crate serde_json;

#[macro_use]
extern crate bson;

#[macro_use]
extern crate log;

use actix_files as fs;
use actix_web::{web, App, HttpServer};
use actix_web::middleware::Logger;
use env_logger::Env;
use std::env;
use crate::search::indexing::index_mods;
use std::fs::File;
use std::path::Path;
use std::io::Write;

mod database;
//mod helpers;
mod models;
mod routes;
mod search;

#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    env_logger::from_env(Env::default().default_filter_or("info")).init();
    dotenv::dotenv().ok();

    let client = database::connect().await.unwrap();

    // Get executable path
    let mut exe_path = env::current_exe()?.parent().unwrap().to_path_buf();
    // Create the path to the index lock file
    exe_path.push("index.v1.lock");

    //Indexing mods if not already done
    if env::args().find(|x| x == "regen").is_some() {
        // User forced regen of indexing
        info!("Forced regeneration of indexes!");
        index_mods(client).await?;
    } else if exe_path.exists() {
        // The indexes were not created, or the version was upgraded
        info!("Indexing of mods for first time...");
        index_mods(client).await?;
        // Create the lock file
        File::create(exe_path)?;
    }


    info!("Starting Actix HTTP server!");

    //Init App
    HttpServer::new(move || {
        App::new()
            .wrap(Logger::default())
            .wrap(Logger::new("%a %{User-Agent}i"))
            .service(routes::index_get)
            //.service(routes::search_post)
            //.service(routes::search_get)
            //.service(routes::mod_page_get)
            //.service(routes::mod_create_get)
            .default_service( web::get().to(routes::not_found))
    })
    .bind("127.0.0.1:8000")?
    .run()
    .await
}
