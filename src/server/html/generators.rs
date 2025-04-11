use byte_unit::Byte;
use byte_unit::UnitType::Binary;
use html::content::Navigation;
use html::inline_text::Anchor;
use html::root::{Body, Html};
use html::tables::builders::TableCellBuilder;
use html::tables::{Table, TableCell, TableRow};
use std::collections::BTreeMap;
use surrealdb::sql;

use crate::db;
use crate::db::{find_down_tree, Entry};
use crate::tools::{entries_for_record, Context, Result};

impl Entry {
	pub async fn make_nav(&self) -> Result<Navigation>
	{
		let mut anchors = Vec::<Anchor>::new();
		let path= find_down_tree(self.id().clone())
			.context(format!("Failed finding parents for {}", self.id()))?;
		for id in path {
			anchors.push(db::lookup(id).await?.unwrap().get_link());
		}

		Ok(Navigation::builder().class("crumbs")
			.ordered_list(|l| {
				l.list_item(|i|i.anchor(|a|
					a.href("/html/studies").text("Studies")
				).class("crumb"));
				anchors.into_iter().rev().fold(l, |l, anchor|
					l.list_item(|i| i.push(anchor).class("crumb"))
				)
			}
			)
			.build())
	}
	pub fn get_link(&self) -> Anchor
	{
		let id = self.id();
		Anchor::builder()
			.href(format!("/html/{}/{}",id.table(),id.str_key()))
			.text(self.name())
			.build()
	}
}

fn table_from_map(map:BTreeMap<String, sql::Value>) -> Table{
	let mut table_builder = Table::builder();
	for (k,v) in map
	{
		table_builder.table_row(|r|{r
			.table_cell(|c|c.text(k))
			.table_cell(|c| {
				match v {
					sql::Value::Object(o) => c.push(table_from_map(o.0)),
					_ => c.text(v.as_raw_string())
				}

			})
		});
	};
	table_builder.build()
}

pub(crate) async fn table_from_objects(
	objs:Vec<Entry>,
	id_name:String,
	keys:Vec<String>,
	additional: Vec<(&str,Box<dyn Fn(&Entry,&mut TableCellBuilder) + Send>)>
) -> Result<Table>
{
	let addkeys:Vec<_> = additional.iter().map(|(k,_)|k.to_string()).collect();

	//build header from the keys (defaults taken from first object)
	let mut table_builder =Table::builder();
	table_builder.table_row(|r|{
		r.table_header(|c|c.text(id_name));
		keys.iter()
			.chain(addkeys.iter())
			.fold(r,|r,key|
				r.table_header(|c|c.text(key.to_owned()))
			)}
	);
	//build rest of the table
	for mut entry in objs //rows
	{
		let addcells:Vec<_> = additional.iter().map(|(_,func)|{
			let mut cell = TableCell::builder();
			func(&entry,&mut cell);
			cell.build()
		}).collect();

		let mut row_builder= TableRow::builder();
		row_builder.table_cell(|c|c.push(entry.get_link()));
		for item in keys.iter().map(|k|entry.remove(k.as_str())) //columns (cells)
		{
			let mut cellbuilder=TableCell::builder();
			if let Some(value) = item
			{
				cellbuilder.text(value.into_inner().to_raw_string());
			} else {cellbuilder.text("----------");}
			row_builder.push(cellbuilder.build());
		}
		row_builder.extend(addcells);
		table_builder.push(row_builder.build());
	}
	Ok(table_builder.build())
}

pub(crate) async fn entry_page(entry:Entry) -> Result<Html>
{
	let mut builder = Body::builder();
	builder.push(entry.make_nav().await?);
	let name = entry.name();
	// @todo this may be very expensive, maybe find a better way
	let common_path= entry.get_path().await?;
	builder.heading_1(|h|h.text(name.to_owned()));
	match entry {
		Entry::Instance((id,mut instance)) => {
			instance.remove("series");
			builder.heading_2(|h|h.text("Attributes"))
				.push(table_from_map(instance.into_inner().0));
			builder.heading_2(|h|h.text("Image"))
				.paragraph(|p|
					p.image(|i|i.src(format!("/api/instances/{}/png",id.str_key())))
				);
		}
		Entry::Series((id,mut series)) => {
			series.remove("study");
			builder.heading_2(|h|h.text("Attributes")).push(table_from_map(series.into_inner().0));
			let mut instances= entries_for_record(&id,"instances").await?;
			instances.sort_by_key(|s|s
				.get_string("number")
				.map_or(0,|s|s
					.parse::<u64>().unwrap_or(0)
				)
			);

			builder
				.heading_2(|t|t.text("Path"))
				.paragraph(|p|p.text(common_path.display().to_string()));

			let keys:Vec<_>=crate::config::get().instance_tags.keys().cloned().collect();
			let makethumb = |obj:&Entry,cell:&mut TableCellBuilder|{
				cell.image(|i|i.src(
					format!("/api/instances/{}/png?width=64&height=64",obj.id().str_key())
				));
			};

			let instance_text = format!("{} Instances",instances.len());
			let instance_table = table_from_objects(instances, "Name".into(), keys, vec![("thumbnail",Box::new(makethumb))]).await?;
			builder.heading_2(|h|h.text(instance_text)).push(instance_table);
		}
		Entry::Study((id,study)) => {
			builder.heading_2(|h|h.text("Attributes")).push(table_from_map(study.into_inner().0));

			let mut series= entries_for_record(&id, "series").await?;
			for s in &mut series
			{
				let v = s.get_instances_per().await?;
				s.insert("Instances", v.count);
				s.insert("Size",format!("{:.2}",Byte::from(v.size).get_appropriate_unit(Binary)));
			}

			builder
				.heading_2(|t|t.text("Path"))
				.paragraph(|p|p.text(common_path.display().to_string()));

			series.sort_by_key(|s|s
				.get_string("Number").expect("missing Number in series")
				.parse::<u64>().expect("Number in Series is not a number")
			);

			let keys= crate::config::get().series_tags.keys().cloned()
				.chain(["Instances","Size"].map(str::to_string)).collect();
			let series_text = format!("{} series",series.len());
			let series_table = table_from_objects(series, "Name".into(), keys, vec![]).await?;
			builder.heading_2(|h|h.text(series_text)).push(series_table);
		}
	}

	Ok(wrap_body(builder.build(), name))
}

pub fn wrap_body<T>(body:Body, title:T) -> Html where T:Into<std::borrow::Cow<'static, str>>
{
	Html::builder().lang("en")
		.head(|h|h
			.title(|t|t.text(title))
			.meta(|m|m.charset("utf-8"))
			.style(|s|s.text(include_str!("styles.css")))
		)
		.push(body)
		.build()
}
