use anyhow::{anyhow, bail, Context};
use html::content::Navigation;
use html::root::{Body,Html};
use html::inline_text::Anchor;
use html::tables::{TableCell,TableRow};
use html::tables::Table;
use surrealdb::sql::Thing;
use crate::db::{Entry,find_down_tree, json_to_thing};
use crate::{JsonMap, JsonVal};
use crate::server::html_item;
use crate::server::html_item::HtmlItem;

struct HtmlEntry(Entry);

impl HtmlEntry
{
	pub fn get_link(&self) ->Anchor
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
	pub async fn query(id:Thing) -> anyhow::Result<HtmlEntry>
	{
		Ok(HtmlEntry{ 0: Entry::query(id).await? })
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

async fn make_nav(entry:Thing) -> anyhow::Result<Navigation>
{
	let mut anchors = Vec::<Anchor>::new();
	let path= find_down_tree(&entry).await
		.context(format!("Failed finding parents for {entry}"))?;
	for id in path {
		anchors.push(HtmlEntry::query(id).await?.get_link());
	}

	Ok(Navigation::builder().class("crumbs")
		.ordered_list(|l|
			anchors.into_iter().rev().fold(l,|l,anchor|
				l.list_item(|i| i.push(anchor).class("crumb"))
			)
		)
		.build())
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
	for mut item in list.into_iter() //rows
	{
		let mut row_builder= TableRow::builder();
		for key in &keys //columns (cells)
		{
			let mut cellbuilder=TableCell::builder();
			if let Some(item) = item.remove(key.as_str())
			{
				match item {
					HtmlItem::Id(id) =>
						cellbuilder.push(HtmlEntry::query(id).await?.get_link()),
					HtmlItem::Array(a) => cellbuilder.text(format!("{} series",a.len())),
					_ => cellbuilder.text(item.to_string())
				};
			}
			row_builder.push(cellbuilder.build());
		}
		table_builder.push(row_builder.build());
	}
	Ok(table_builder.build())
}

pub(crate) async fn make_entry_page(mut entry:JsonMap) -> anyhow::Result<Html>
{
	let id = json_to_thing(entry.remove("id").expect("entry should have an id"))?;
	let mut builder = Body::builder();
	let name = Entry::query(id).await?.get_name();
	builder.heading_1(|h|h.text(name));
	Ok(wrap_body(builder.build(), "Studies"))
}
