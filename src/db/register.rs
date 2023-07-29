use std::collections::BTreeMap;
use surrealdb::{Connection, Surreal, Result, Error};
use crate::db::{DbVal, Entry};

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
		let result = db.query("if SELECT <-contains FROM $contained then [] else RELATE $container->contains->$contained return none end")
			.bind(("contained",contained.id.clone()))
			.bind(("container",container.id.clone()))
			.await?.check()?;
	}
	Ok(())
}
async fn touch_entry<C>(db:&Surreal<C>,mut data:BTreeMap<String,DbVal>) -> Result<Entry> where C: Connection{
	let DbVal::Thing(id) = data.remove("id").expect("Data is missing \"id\"")
		else {panic!("\"id\" in data is not an id")};

	match db.create(id.clone()).content(data).await
	{
		Ok(data) => Ok(Entry { id, data, created: true }),
		Err(e) => match e {
			Error::Api(ref apierr) => match apierr {
				surrealdb::error::Api::Query(_) => Ok(Entry {
					id: id.clone(),
					data: db.select(id.clone()).await?,
					created: false,
				}),
				_ => Err(e)
			},
			_ => Err(e)
		}
	}
}
