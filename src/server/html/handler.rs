use crate::db;
use crate::db::{list_entries, AggregateData, Entry, RecordId, DB};
use crate::server::html::generators;
use crate::server::http_error::{HttpError, IntoHttpError};
use axum::extract::{Path, Query};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use byte_unit::Byte;
use byte_unit::UnitType::Binary;
use html::root::Body;
use html::tables::builders::TableCellBuilder;
use itertools::Itertools;
use serde::Deserialize;
use serde_json::json;
use std::cmp::Ordering;
use std::collections::BTreeMap;

#[derive(Deserialize)]
pub struct ListingConfig {
    filter:Option<String>,
    sort_by:Option<String>,
    #[serde(default)]
    sort_reverse:bool
}


pub async fn get_studies_html(headers: HeaderMap,Query(config): Query<ListingConfig>) -> Result<axum::response::Html<String>, HttpError>
{
    let keys:Vec<_> = crate::config::get().study_tags.keys().cloned()
        .chain(["Date", "Time"].map(String::from))
        .unique()//make sure there are no duplicates
        .collect();

    let mut studies = list_entries("studies").await.into_http_error(&headers)?;

    if let Some(filter) = config.filter
    {
        studies.retain(|e|e.name().find(filter.as_str()).is_some());
    }

    let sortby = config.sort_by.unwrap_or("Date".to_string());
    studies.sort_by(|e1,e2|
        match (e1.get(&sortby),e2.get(&sortby)) {
            (Some(v1),Some(v2)) => if config.sort_reverse {
                v1.partial_cmp(v2).unwrap_or(Ordering::Equal)
            } else {
                v2.partial_cmp(v1).unwrap_or(Ordering::Equal)
            },
            _ => Ordering::Equal
        }
    );

    // get some aggregated data
    let aggregate_instances:Vec<AggregateData> = DB.select("instances_per_studies").await
        .into_http_error(&headers)?;
    // collect results from above
    let instance_count:BTreeMap<RecordId,_> = aggregate_instances.iter()
        .map(|e|(e.get_inner_id(),e.count)).collect();
    let filesizes:BTreeMap<RecordId,_> =aggregate_instances.into_iter()
        .map(|e|(e.get_inner_id(),Byte::from(e.size))).collect();

    let countinstances = move |obj:&Entry,cell:&mut TableCellBuilder| {
        let inst_cnt= instance_count[obj.id()];
        cell.text(inst_cnt.to_string());
    };
    let getfilesize = move |obj:&Entry,cell:&mut TableCellBuilder|{
        cell.text(format!("{:.2}",filesizes[&obj.id()].get_appropriate_unit(Binary)));
    };

    let table= generators::table_from_objects(
        studies, "Name".to_string(), keys,
        vec![("Instances",Box::new(countinstances)),("Size",Box::new(getfilesize))]
    ).await.map_err(|e|e.context("Failed generating the table")).into_http_error(&headers)?;

    let mut builder = Body::builder();
    builder.heading_1(|h|h.text("Studies"));
    builder.push(table);
    Ok(axum::response::Html(generators::wrap_body(builder.build(), "Studies").to_string()))
}

pub async fn get_entry_html(headers: HeaderMap,Path((table,id)):Path<(String,String)>) -> Result<Response, HttpError>
{
    if let Some(entry) = db::lookup_uid(table,id).await.into_http_error(&headers)?
    {
        let page = generators::entry_page(entry).await.into_http_error(&headers)?;
        Ok(axum::response::Html(page.to_string()).into_response())
    }
    else
    {
        Ok((StatusCode::NOT_FOUND,Json(json!({"Status":"not found"}))).into_response())
    }
}

