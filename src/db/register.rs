use std::collections::BTreeMap;
use surrealdb::{Connection, Surreal, Result};
use crate::db::{DbVal, Entry, JsonValue};

pub async fn register<C>(db:&Surreal<C>,
						 instance_meta:BTreeMap<String,DbVal>,
						 series_meta:BTreeMap<String,DbVal>,
						 study_meta: BTreeMap<String, DbVal>
) -> Result<Entry>
	where C: Connection
{
	let study = touch_entry(db,study_meta).await?;
	let series= touch_entry(db,series_meta).await?;
	let instance=touch_entry(db,instance_meta).await?;

	touch_relate(db,&study,&series).await?;
	touch_relate(db,&series, &instance).await?;
	Ok(instance)
}

async fn touch_relate<C>(db:&Surreal<C>,container:&Entry,contained:&Entry) -> Result<()> where C: Connection {
	if contained.created {
		let result = db.query("RELATE $container->contains->$contained return none")
			.bind(("contained",contained.id.clone()))
			.bind(("container",container.id.clone()))
			.await?.check()?;
	}
	Ok(())
}
async fn touch_entry<C>(db:&Surreal<C>,mut data:BTreeMap<String,DbVal>) -> Result<Entry> where C: Connection{
	let DbVal::Thing(id) = data.remove("id").expect("Data is missing \"id\"")
		else {panic!("\"id\" in data is not an id")};

	let mut res= db
		.query("if select id from $id then [False,select * from $id] else [True,create $id content $data] end")
		.bind(("id",id.clone()))
		.bind(("data",data))
		.await?.check()?;
	let query_ret:Vec<JsonValue> = res.take(0)?;
	Ok(Entry{
		id,
		data: query_ret.get(1).unwrap().get(0).unwrap().clone(),
		created: query_ret.get(0).unwrap().as_bool().unwrap(),
	})
}
