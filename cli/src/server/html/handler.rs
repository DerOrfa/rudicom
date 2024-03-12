use std::cmp::Ordering;
use std::collections::HashMap;
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::Json;
use axum::response::{IntoResponse, Response};
use byte_unit::UnitType::Binary;
use html::root::Body;
use html::tables::builders::TableCellBuilder;
use itertools::Itertools;
use serde::Deserialize;
use serde_json::json;
use tokio::task::JoinSet;
use crate::db;
use crate::db::Entry;
use crate::server::html::generators;
use crate::server::http_error::TextError;

#[derive(Deserialize)]
pub(crate) struct ListingConfig {
    filter:Option<String>,
    sort_by:Option<String>,
    #[serde(default)]
    sort_reverse:bool
}


pub(crate) async fn get_studies_html(Query(config): Query<ListingConfig>) -> Result<axum::response::Html<String>,TextError>
{
    let keys=["StudyDate", "StudyTime"].into_iter().map(|s|s.to_string())
        .chain(crate::config::get::<Vec<String>>("study_tags").unwrap().into_iter())//get the rest from the config
        .unique()//make sure there are no duplicates
        .collect();

    let mut studies = db::list_table("studies").await?;
    if let Some(filter) = config.filter
    {
        studies.retain(|e|e.name().find(filter.as_str()).is_some());
    }
    
    if let Some(sortby) = config.sort_by
    {
        let sortby= sortby.as_str();
        studies.sort_by(|e1,e2| 
            match (e1.get(sortby),e2.get(sortby)) {
                (Some(v1),Some(v2)) => if config.sort_reverse {v1.cmp(v2)} else {v2.cmp(v1)},
                _ => Ordering::Equal
            }
        )
    }
    

    // count instances before as db::list_children cant be used in a closure / also parallelization
    let mut counts=JoinSet::new();
    for stdy in studies.iter().map(Entry::clone)
    {
        counts.spawn(async move {
            let id = stdy.id().clone();
            let inst_cnt= db::list_values(&id, "series.instances",true).await
                .map(|l|l.len())?;
            let size = stdy.size().await?;
            crate::tools::Result::Ok((id,inst_cnt,size))
        });
    }
    // collect results from above
    let mut instance_count : HashMap<_,_> = HashMap::new();
    let mut filesizes : HashMap<_,_> = HashMap::new();
    while let Some(res) = counts.join_next().await
    {
        let (k,v,s) = res??;
        instance_count.insert(k.clone(),v);
        filesizes.insert(k,s);
    }
    let countinstances = move |obj:&Entry,cell:&mut TableCellBuilder| {
        let inst_cnt=instance_count[obj.id()];
        cell.text(inst_cnt.to_string());
    };
    let getsize = move |obj:&Entry,cell:&mut TableCellBuilder| {
        cell.text(format!("{:.2}",filesizes[obj.id()].get_appropriate_unit(Binary)));
    };

    let table= generators::table_from_objects(
        studies, "Study".to_string(), keys,
        vec![("Instances",Box::new(countinstances)),("Size",Box::new(getsize))]
    ).await.map_err(|e|e.context("Failed generating the table"))?;

    let mut builder = Body::builder();
    builder.heading_1(|h|h.text("Studies"));
    builder.push(table);
    Ok(axum::response::Html(generators::wrap_body(builder.build(), "Studies").to_string()))
}
pub(crate) async fn get_entry_html(Path(id):Path<(String,String)>) -> Result<Response,TextError>
{
    if let Some(entry) = db::lookup(&id.into()).await?
    {
        let page = generators::entry_page(entry).await?;
        Ok(axum::response::Html(page.to_string()).into_response())
    }
    else
    {
        Ok((StatusCode::NOT_FOUND,Json(json!({"Status":"not found"}))).into_response())
    }
}

