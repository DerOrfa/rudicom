use std::collections::BTreeMap;
use surrealdb::{Connection, Surreal,Result};
use surrealdb::sql::Value as DbVal;
use crate::db::{Entry, JsonValue};



pub async fn register_manual<C>(db:&Surreal<C>,
							   mut instance_meta:BTreeMap<String,DbVal>,
							   mut series_meta:BTreeMap<String,DbVal>,
							   mut study_meta: BTreeMap<String, DbVal>
) -> surrealdb::Result<serde_json::Value>
	where C: Connection
{
	let study = touch_entry(db,study_meta.clone()).await?;
	let series= touch_entry(db,series_meta).await?;
	let instance=touch_entry(db,instance_meta).await?;

	touch_relate(db,&study,&series);
	touch_relate(db,&series, &instance);
	if instance.created {Ok(JsonValue::Null)} else { Ok(instance.into()) }
}

async fn touch_relate<C>(db:&Surreal<C>,container:&Entry,contained:&Entry) -> Result<()> where C: Connection {
	if contained.created {
		db.query("RELATE $container->contains->$contained return none")
			.bind(("contained",contained.id.clone()))
			.bind(("container",container.id.clone()))
			.await?.check()?;
	}
	Ok(())
}
async fn touch_entry<C>(db:&Surreal<C>,mut data:BTreeMap<String,DbVal>) -> Result<Entry> where C: Connection{
	let DbVal::Thing(id) = data.remove("id").expect("Data is missing \"id\"")
		else {panic!("\"id\" in data is not an id")};

	let found:JsonValue = db.select(id.clone()).await?;
	match found {
		JsonValue::Null =>
			db.create(id.clone()).content(data).await.map(|data|Entry{ id, data, created: true }),
		_ => Ok(Entry{ id, data: found, created: false })
	}
}
