use std::io::Result as IoResult;
use actix_web::{App, HttpServer};
use sqlx::PgPool;
use tokio::sync::OnceCell;

use course::page as course_page;
use search::page as search_page;

mod course;
mod search;

static CONNECTION: OnceCell<PgPool> = OnceCell::const_new();

#[actix_web::main]
async fn main() -> IoResult<()> {
    CONNECTION.get_or_init(|| async {
        PgPool::connect(include_str!("../../connection_string"))
            .await
            .expect("failed to connect to db")
    }).await;

    HttpServer::new(||
        App::new()
            .service(search_page)
            .service(course_page)
            // TODO: error and 404
    )
        .bind(("127.0.0.1", 8080))?
        .run()
        .await
}
