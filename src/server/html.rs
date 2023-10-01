use std::collections::LinkedList;
use anyhow::{anyhow, bail, Context};
use html::content::Navigation;
use html::root::{Body,Html};
use html::inline_text::Anchor;
use html::tables::{TableCell,TableRow};
use html::tables::builders::TableCellBuilder;
use html::tables::Table;
use serde_json::Value;
use surrealdb::sql::Thing;
use crate::db::{Entry,find_down_tree, json_to_thing};
use crate::{db, JsonMap, JsonVal};
use crate::server::html_item::HtmlItem;

pub(crate) struct HtmlEntry(Entry);

impl HtmlEntry
{
	pub fn get_link(&self) -> Anchor
	{
		let typename = match self.0 {
			Entry::Instance(_) => "instances",
			Entry::Series(_) => "series",
			Entry::Study(_) => "studies"
		};
		Anchor::builder()
			.href(format!("/{typename}/{}/html",self.0.get_id()))
			.text(self.0.get_name())
			.build()
	}
	pub fn into_items(self, keys: &Vec<String>) -> anyhow::Result<LinkedList<(String,HtmlItem)>>
	{
		let link = self.get_link();
		let mut out:LinkedList<(String,HtmlItem)> = LinkedList::new();
		let mut inmap = match self.0 {
			Entry::Instance(map) => map,
			Entry::Series(map) => map,
			Entry::Study(map) => map,
		};
		for k in keys
		{
			let item = match inmap.remove(k.as_str()).unwrap_or(JsonVal::String("-------".to_string())) {
				JsonVal::Bool(b) => HtmlItem::Bool(b),
				JsonVal::Number(n) => HtmlItem::Number(n),
				JsonVal::String(s) => HtmlItem::String(s),
				JsonVal::Array(a) => HtmlItem::String(format!("{} children", a.len())),
				JsonVal::Object(o) => {
					let jsonval=JsonVal::Object(o);
					if let Some(id) = json_to_thing(jsonval.clone()).ok() {
						HtmlItem::Id((id, link.clone()))
					} else {
						HtmlItem::String(jsonval.to_string())
					}
				}
				_ => bail!("invalid value in {k}"),
			};
			out.push_back((k.clone(), item));
		}
		Ok(out)
	}

	pub async fn query(id:Thing) -> anyhow::Result<HtmlEntry>
	{
		Ok(HtmlEntry{ 0: Entry::query(id).await? })
	}
}

impl TryFrom<JsonMap> for HtmlEntry
{
	type Error = anyhow::Error;

	fn try_from(json_entry: JsonMap) -> std::result::Result<Self, Self::Error>
	{
		Ok(HtmlEntry{ 0: Entry::try_from(json_entry)? })
	}
}

pub fn wrap_body<T>(body:Body, title:T) -> Html where T:Into<std::borrow::Cow<'static, str>>
{
	Html::builder().lang("en")
		.head(|h|h
			.title(|t|t.text(title))
			.meta(|m|m.charset("utf-8"))
			.style(|s|s
				.text(r#"nav {border-bottom: 1px solid black;}"#)
				.text(r#".crumbs ol {list-style-type: none;padding-left: 0;}"#)
				.text(r#".crumb {display: inline-block;}"#)
				.text(r#".crumb a::after {display: inline-block;color: #000;content: '>'; font-size: 80%;font-weight: bold;padding: 0 3px;}"#)
				.text(r#"table {border-collapse: collapse; border: 2px solid rgb(200,200,200); letter-spacing: 1px; font-size: 0.8rem;}"#)
				.text(r#"td, th {border: 1px solid rgb(190,190,190); padding: 10px 20px;}"#)
				.text(r#"th {background-color: rgb(235,235,235);}"#)
				.text(r#"td {text-align: center;}"#)
				.text(r#"tr:nth-child(even) td {background-color: rgb(250,250,250);}"#)
				.text(r#"tr:nth-child(odd) td {background-color: rgb(245,245,245);}"#)
				.text(r#"caption {padding: 10px;}"#)
			)
		)
		.push(body)
		.build()
}

async fn make_nav(entry:&Thing) -> anyhow::Result<Navigation>
{
	let mut anchors = Vec::<Anchor>::new();
	let path= find_down_tree(&entry).await
		.context(format!("Failed finding parents for {entry}"))?;
	for id in path {
		anchors.push(HtmlEntry::query(id).await?.get_link());
	}


	Ok(Navigation::builder().class("crumbs")
		.ordered_list(|l| {
			l.list_item(|i|i.anchor(|a|
				a.href("/studies/html").text("Studies")
			).class("crumb"));
			anchors.into_iter().rev().fold(l, |l, anchor|
				l.list_item(|i| i.push(anchor).class("crumb"))
			)
		}
		)
		.build())
}

fn make_table_from_map(map:JsonMap) -> Table{
	let mut table_builder = Table::builder();
	for (k,v) in map
	{
		table_builder.table_row(|r|{r
			.table_cell(|c|c.text(k))
			.table_cell(|c|c.text(v.to_string()))
		});
	};
	table_builder.build()
}

pub(crate) async fn make_table_from_objects(
	objs:Vec<JsonVal>,
	id_name:String,
	mut keys:Vec<String>,
	additional: Vec<(&str,fn(&HtmlEntry,&mut TableCellBuilder))>
) -> anyhow::Result<Table>
{
	// make sure we have a proper list
	if objs.is_empty(){bail!("Empty list")}
	let addkeys:Vec<_> = additional.iter().map(|(k,_)|k.to_string()).collect();

	//build header from the keys (defaults taken from first json-object)
	let mut table_builder =Table::builder();
	table_builder.table_row(|r|{
		r.table_header(|c|c.text(id_name));
		keys.iter()
			.chain(addkeys.iter())
			.fold(r,|r,key|
				r.table_header(|c|c.text(key.to_owned()))
			)}
	);
	//sneak in "id" so we will iterate through it (and query it) when building the rest of the table
	keys.insert(0,"id".to_string());
	let list:Result<Vec<_>,_> = objs.into_iter()
		.map(|v| {
			match v {
				Value::Object(o) => {
					HtmlEntry::try_from(o)
						.context(anyhow!("Failed parsing list of db-entries"))
				}
				_ => {Err(anyhow!("json value {v} must be an object"))}
			}
		})
		.collect();
	let list = list?;
	//build rest of the table
	for entry in list.into_iter() //rows
	{
		let addcells:Vec<_> = additional.iter().map(|(key,func)|{
			let mut cell = TableCell::builder();
			func(&entry,&mut cell);
			cell.build()
		}).collect();

		let mut row_builder= TableRow::builder();
		for (_,item) in entry.into_items(&keys)? //columns (cells)
		{
			let mut cellbuilder=TableCell::builder();
			match item {
				HtmlItem::Id((_,link)) => cellbuilder.push(link),
				_ => cellbuilder.text(item.to_string())
			};
			row_builder.push(cellbuilder.build());
		}
		row_builder.extend(addcells);
		table_builder.push(row_builder.build());
	}
	Ok(table_builder.build())
}

pub(crate) async fn make_entry_page(mut entry:JsonMap) -> anyhow::Result<Html>
{
	let id = json_to_thing(entry.remove("id").expect("entry should have an id"))?;
	let mut builder = Body::builder();
	builder.push(make_nav(&id).await?);
	let mut entry = Entry::query(id.clone()).await?;
	let name = entry.get_name();
	entry.remove("id");
	builder.heading_1(|h|h.text(name.to_owned()));
	match entry {
		Entry::Instance(mut instance) => {
			let _file = instance.remove("file");
			instance.remove("series");
			builder.push(make_table_from_map(instance));
			builder.paragraph(|p|
				p.image(|i|i.src(format!("/instances/{}/png",id.id.to_raw())))
			);
		}
		Entry::Series(mut series) => {
			series.remove("instances");
			series.remove("study");
			builder.push(make_table_from_map(series));
			let mut instances=db::list_children(id,"instances").await?;
			instances.sort_by_key(|s|s
				.get("InstanceNumber").expect("missing InstanceNumber").as_str().unwrap()
				.parse::<u64>().expect("InstanceNumber is not a number")
			);

			let keys=crate::config::get::<Vec<String>>("instance_tags")?;
			let makethumb = |obj:&HtmlEntry,cell:&mut TableCellBuilder|{
				cell.image(|i|i.src(
					format!("/instances/{}/png?width=64&height=64",obj.0.get_id())
				));
			};
			let instance_text = format!("{} Instances",instances.len());
			let instance_table = make_table_from_objects(instances, "Name".into(), keys, vec![("thumbnail",makethumb)]).await?;
			builder.heading_2(|h|h.text(instance_text)).push(instance_table);
		}
		Entry::Study(mut study) => {
			study.remove("series");
			builder.push(make_table_from_map(study));
			let mut series=db::list_children(id,"series").await?;
			series.sort_by_key(|s|s
					.get("SeriesNumber").expect("missing SeriesNumber").as_str().unwrap()
					.parse::<u64>().expect("SeriesNumber is not a number")
			);

			let keys= crate::config::get::<Vec<String>>("series_tags")?;
			let series_text = format!("{} Series",series.len());
			let series_table = make_table_from_objects(series, "Name".into(), keys, [].into()).await?;
			builder.heading_2(|h|h.text(series_text)).push(series_table);
		}
	}

	Ok(wrap_body(builder.build(), name))
}
