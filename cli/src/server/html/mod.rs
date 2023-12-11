mod handler;
mod generators;

use axum::routing::get;

	// pub fn into_items(mut self, keys: &Vec<String>) -> anyhow::Result<LinkedList<(String,HtmlItem)>>
	// {
	// 	let link = self.get_link();
	// 	let mut out:LinkedList<(String,HtmlItem)> = LinkedList::new();
	// 	for k in keys
	// 	{
	// 		let item = match self.0.remove(k.as_str()).unwrap_or(sql::Value::from("-------")).into_json()
	// 		{
	// 			JsonVal::Bool(b) => HtmlItem::Bool(b),
	// 			JsonVal::Number(n) => HtmlItem::Number(n),
	// 			JsonVal::String(s) => HtmlItem::String(s),
	// 			JsonVal::Array(a) => HtmlItem::String(format!("{} children", a.len())),
	// 			JsonVal::Object(o) => {
	// 				let jsonval=JsonVal::Object(o);
	// 				if let Some(id) = json_to_thing(jsonval.clone()).ok() {
	// 					HtmlItem::Id((id, link.clone()))
	// 				} else {
	// 					HtmlItem::String(jsonval.to_string())
	// 				}
	// 			}
	// 			_ => bail!("invalid value in {k}"),
	// 		};
	// 		out.push_back((k.clone(), item));
	// 	}
	// 	Ok(out)
	// }

pub(crate) fn router() -> axum::Router
{
	axum::Router::new()
		.route("/studies/html",get(handler::get_studies_html))
		.route("/:table/:id/html",get(handler::get_entry_html))
}
