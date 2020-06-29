mod mod_item;
mod version_item;

pub use mod_item::Mod;
pub use version_item::Version;
use mongodb::Database;
use crate::database::Result;
use bson::{Document, Bson};
use crate::database::DatabaseError::NotFound;
use async_trait::async_trait;

#[async_trait]
pub trait Item {
    fn get_collection() -> &'static str;
    async fn get_by_id(client: Database, id: &str) -> Result<Box<Self>> {
        let filter = doc! { "_id": id };
        let collection = client.collection(Self::get_collection());
        let doc : Document = match collection.find_one(filter, None).await? {
            Some(e) => e,
            None => return Err(NotFound())
        };
        let elem: Box<Self> = Self::from_doc(doc)?;
        Ok(elem)
    }
    fn from_doc(elem: Document) -> Result<Box<Self>>;
}