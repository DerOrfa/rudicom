use anyhow::{anyhow, bail, Context};
use html::content::Navigation;
use html::root::{Body,Html};
use html::inline_text::Anchor;
use html::tables::{TableCell,TableRow};
use html::tables::Table;
use surrealdb::sql::Thing;
use crate::db::{find_down_tree, query_for_entry};
use crate::JsonVal;
use crate::server::html_item;
use crate::server::html_item::HtmlItem;

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

async fn make_entry_link(entry:&Thing) ->Anchor
{
	let mut builder= html::inline_text::Anchor::builder();
	match query_for_entry(entry.to_owned()).await
	{
		Ok(data)=> {
			match entry.tb.as_str() {
				"instances" => {
					let number=data.get("InstanceNumber").and_then(|v|v.as_str()).unwrap_or("--");

					builder
						.href(format!("/instances/{}/html",entry.id.to_raw()))
						.text(format!("Instance {number}"));
				},
				"series" => {
					let number=data.get("SeriesNumber").and_then(|v|v.as_str()).unwrap_or("--");
					let desc= data.get("SeriesDescription").and_then(|v|v.as_str()).unwrap_or("--");

					builder
						.href(format!("/series/{}/html",entry.id.to_raw()))
						.text(format!("S{number}_{desc}"));
				},
				"studies" => {
					let id=data.get("PatientID").and_then(|v|v.as_str()).unwrap_or("--");
					let date=data.get("StudyDate").and_then(|v|v.as_str()).unwrap_or("--");
					let time=data.get("StudyTime").and_then(|v|v.as_str()).unwrap_or("--");

					builder
						.href(format!("/studies/{}/html",entry.id.to_raw()))
						.text(format!("{id}/{date}_{time}"));
				}
				_ => {tracing::error!("invalid db table name when generating a link");}
			}
		},
		Err(e)=>{
			tracing::error!("querying {entry} failed when constructing a link ({e})");
		}
	}
	builder.build()
}

async fn make_nav(entry:Thing) -> Navigation
{
	let mut anchors = Vec::<Anchor>::new();
	match find_down_tree(&entry).await{
		Ok(path) => {
			for id in &path{
				anchors.push(make_entry_link(id).await);
			}
		}
		Err(e) => {tracing::error!("Failed finding parents for {entry}:{e}")}
	};

	Navigation::builder().class("crumbs")
		.ordered_list(|l|
			anchors.into_iter().rev().fold(l,|l,anchor|
				l.list_item(|i| i.push(anchor).class("crumb"))
			)
		)
		.build()
}

pub(crate) async fn make_table(list:Vec<JsonVal>,id_name:String, mut keys:Vec<String>) -> anyhow::Result<Table>
{
	// make sure we have a proper list
	if list.is_empty(){bail!("Empty list")}
	let list:Result<Vec<_>,_> = list.into_iter()
		.map(|v|html_item::make_item_map(v).context(anyhow!("Should be a list of objects")))
		.collect();
	let list = list?;

	//build header from the keys (defaults taken from first json-object)
	let mut table_builder =Table::builder();
	table_builder.table_row(|r|{
		r.table_header(|c|c.text(id_name));
		keys.iter().fold(r,|r,key|
			r.table_header(|c|c.text(key.to_owned()))
		)}
	);
	//sneak in "id" so we will iterate through it (and query it) when building the rest of the table
	keys.insert(0,"id".to_string());
	//build rest of the table
	for item in list //rows
	{
		let mut row_builder= TableRow::builder();
		for key in &keys //columns (cells)
		{
			let mut cellbuilder=TableCell::builder();
			if let Some(item) = item.get(key.as_str())
			{
				match item {
					HtmlItem::Id(id) => { cellbuilder.push(make_entry_link(id).await);},
					HtmlItem::Array(a) => {cellbuilder.text(format!("{} series",a.len()));},
					_ => {cellbuilder.text(item.to_string());}
				}
			}
			row_builder.push(cellbuilder.build());
		}
		table_builder.push(row_builder.build());
	}
	Ok(table_builder.build())
}
