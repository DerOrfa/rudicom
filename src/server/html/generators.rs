use std::collections::BTreeMap;
use anyhow::{bail, Context};
use html::content::Navigation;
use html::inline_text::Anchor;
use html::root::{Body, Html};
use html::tables::builders::TableCellBuilder;
use html::tables::{Table, TableCell, TableRow};
use surrealdb::sql;
use surrealdb::sql::Value;
use crate::db;
use crate::db::{Entry, find_down_tree};
use crate::tools::{lookup_instance_filepath, reduce_path};

impl Entry {
    pub async fn make_nav(&self) -> anyhow::Result<Navigation>
    {
        let mut anchors = Vec::<Anchor>::new();
        let path= find_down_tree(self.id()).await
            .context(format!("Failed finding parents for {}", self.id()))?;
        for id in path {
            anchors.push(db::lookup(&id).await?.unwrap().get_link());
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
    pub fn get_link(&self) -> Anchor
    {
        let id = self.id();
        Anchor::builder()
            .href(format!("/{}/{}/html",id.tb,id.id.to_raw()))
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
            .table_cell(|c|c.text(v.to_string()))
        });
    };
    table_builder.build()
}

pub(crate) async fn table_from_objects<F>(
    objs:Vec<Entry>,
    id_name:String,
    keys:Vec<String>,
    additional: Vec<(&str,F)>
) -> anyhow::Result<Table> where F:Fn(&Entry,&mut TableCellBuilder)
{
    // make sure we have a proper list
    if objs.is_empty(){bail!("Empty list")}
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
    for entry in objs //rows
    {
        let addcells:Vec<_> = additional.iter().map(|(_,func)|{
            let mut cell = TableCell::builder();
            func(&entry,&mut cell);
            cell.build()
        }).collect();

        let mut row_builder= TableRow::builder();
        row_builder.table_cell(|c|c.push(entry.get_link()));
        for item in keys.iter().map(|k|entry.get(k.as_str())) //columns (cells)
        {
            let mut cellbuilder=TableCell::builder();
            if let Some(value) = item
            {
                cellbuilder.text(value.to_string());
            } else {cellbuilder.text("----------");}
            row_builder.push(cellbuilder.build());
        }
        row_builder.extend(addcells);
        table_builder.push(row_builder.build());
    }
    Ok(table_builder.build())
}

pub(crate) async fn entry_page(entry:Entry) -> anyhow::Result<Html>
{
    let mut builder = Body::builder();
    builder.push(entry.make_nav().await?);
    let name = entry.name();
    builder.heading_1(|h|h.text(name.to_owned()));
    match entry {
        Entry::Instance((id,mut instance)) => {
            if let Some(filepath)=lookup_instance_filepath(id.id.to_raw().as_str()).await?{
                builder
                    .heading_2(|t|t.text("filename"))
                    .paragraph(|p|p.text(filepath.to_string_lossy().to_string()));
            }

            instance.remove("series");
            builder.heading_2(|h|h.text("Attributes"))
                .push(table_from_map(instance.0));
            builder.heading_2(|h|h.text("Image"))
                .paragraph(|p|
                    p.image(|i|i.src(format!("/instances/{}/png",id.id.to_raw())))
                );
        }
        Entry::Series((id,mut series)) => {
            series.remove("instances");
            series.remove("study");
            builder.heading_2(|h|h.text("Attributes")).push(table_from_map(series.0));
            let mut instances=db::list_children(&id, "instances").await?;
            instances.sort_by_key(|s|s
                .get_string("InstanceNumber").expect("missing InstanceNumber")
                .parse::<u64>().expect("InstanceNumber is not a number")
            );
            let files:Vec<_> = instances.iter_mut()
                .filter_map(|v|v.remove("file"))
                .filter_map(|f|db::File::try_from(f).ok())
                .map(|f|f.get_path())
                .collect();

            let path = reduce_path(files);
            builder
                .heading_2(|t|t.text("Path"))
                .paragraph(|p|p.text(path.to_string_lossy().to_string()));


            let keys=crate::config::get::<Vec<String>>("instance_tags")?;
            let makethumb = |obj:&Entry,cell:&mut TableCellBuilder|{
                cell.image(|i|i.src(
                    format!("/instances/{}/png?width=64&height=64",obj.id().id.to_raw())
                ));
            };
            let instance_text = format!("{} Instances",instances.len());
            let instance_table = table_from_objects(instances, "Name".into(), keys, vec![("thumbnail",makethumb)]).await?;
            builder.heading_2(|h|h.text(instance_text)).push(instance_table);
        }
        Entry::Study((id,mut study)) => {
            study.remove("series");
            builder.heading_2(|h|h.text("Attributes")).push(table_from_map(study.0));

            let mut series=db::list_children(&id, "series").await?;
            // get flat list of file-attributes
            // let files:Vec<_>=db::list_children(id,"series.instances.file").await?.into_iter()
            // 	.filter_map(|v|if let JsonVal::Array(array)=v{Some(array)} else {None})
            // 	.flatten().collect();
            // makes PathBuf of them
            // let files:anyhow::Result<Vec<_>>=files.iter()
            // 	.filter_map(|f|f.as_object())
            // 	.map(|o|json_to_path(o))
            // 	.collect();
            // reduce them ant print them @todo this is very expensive, maybe find a better way
            // if let Ok(path) = files.map(reduce_path)
            // {
            // 	builder
            // 		.heading_2(|t|t.text("Path"))
            // 		.paragraph(|p|p.text(path.to_string_lossy().to_string()));
            // }

            series.sort_by_key(|s|s
                .get_string("SeriesNumber").expect("missing SeriesNumber")
                .parse::<u64>().expect("SeriesNumber is not a number")
            );
            let countinstances = |obj:&Entry,cell:&mut TableCellBuilder|{
                if let Some(len)= obj
                    .get("instances")
                    .and_then(|v|if let Value::Array(a) = v {Some(a)} else {None} )
                    .map(|l|l.len())
                {
                    cell.text(format!("{len} instances"));
                }
            };

            let keys= crate::config::get::<Vec<String>>("series_tags")?;
            let series_text = format!("{} Series",series.len());
            let series_table = table_from_objects(series, "Name".into(), keys, vec![("Instances",countinstances)]).await?;
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
