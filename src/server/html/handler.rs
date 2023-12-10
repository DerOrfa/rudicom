use std::collections::HashMap;
use axum::extract::{Path, Query};
use axum::http::StatusCode;
use axum::Json;
use axum::response::{IntoResponse, Response};
use html::root::Body;
use html::tables::builders::TableCellBuilder;
use itertools::Itertools;
use serde::Deserialize;
use serde_json::json;
use surrealdb::sql;
use tokio::task::JoinSet;
use crate::db;
use crate::db::Entry;
use crate::server::html::generators;
use crate::server::TextError;

#[derive(Deserialize)]
pub(crate) struct StudyFilter {
    filter: String,
}

pub(crate) async fn get_studies_html(filter: Option<Query<StudyFilter>>) -> Result<axum::response::Html<String>,TextError>
{
    let keys=["StudyDate", "StudyTime"].into_iter().map(|s|s.to_string())
        .chain(crate::config::get::<Vec<String>>("study_tags").unwrap().into_iter())//get the rest from the config
        .unique()//make sure there are no duplicates
        .collect();

    let mut studies = db::list_table("studies").await?;
    if let Some(filter) = filter
    {
        studies.retain(|e|e.name().find(filter.filter.as_str()).is_some());
    }

    // count instances before as db::list_children cant be used in a closure / also parallelization
    let mut counts=JoinSet::new();
    for id in studies.iter().map(|e|e.id().clone())
    {
        counts.spawn(async move {
            db::list_values(&id, "series.instances",true).await
                .map(|l|(id,l.len()))
        });
    }
    // collect results from above
    let mut instance_count : HashMap<_,_> = HashMap::new();
    while let Some(res) = counts.join_next().await
    {
        let (k,v) = res??;
        instance_count.insert(k,v);
    }
    let countinstances = |obj:&Entry,cell:&mut TableCellBuilder| {
        let inst_cnt=instance_count[obj.id()];
        cell.text(inst_cnt.to_string());
    };

    let table= generators::table_from_objects(
        studies, "Study".to_string(), keys,
        vec![("Instances",countinstances)]
    ).await.map_err(|e|e.context("Failed generating the table"))?;

    let mut builder = Body::builder();
    builder.heading_1(|h|h.text("Studies"));
    builder.push(table);
    Ok(axum::response::Html(generators::wrap_body(builder.build(), "Studies").to_string()))
}
pub(crate) async fn get_entry_html(Path((table,id)):Path<(String,String)>) -> Result<Response,TextError>
{
    let id = sql::Thing::from((table, id));
    if let Some(entry) = db::lookup(&id).await?
    {
        let page = generators::entry_page(entry).await?;
        Ok(axum::response::Html(page.to_string()).into_response())
    }
    else
    {
        Ok((StatusCode::NOT_FOUND,Json(json!({"Status":"not found"}))).into_response())
    }
}

