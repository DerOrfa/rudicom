use html::inline_text::Anchor;
use surrealdb::sql;
use surrealdb::sql::Value;

pub(crate) enum HtmlItem {
	Bool(bool),
	Number(serde_json::Number),
	String(String),
	Id((sql::Thing,Anchor))
}

impl TryFrom<sql::Value> for HtmlItem
{
	type Error = anyhow::Error;

	fn try_from(value: Value) -> Result<Self, Self::Error> {
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
		_ => bail!("invalid value in {k}")
	}
}
impl ToString for HtmlItem
{
	fn to_string(&self) -> String {
		match self {
			HtmlItem::Bool(b) => if *b {"True".into()} else {"False".into()},
			HtmlItem::Number(n) => n.to_string(),
			HtmlItem::String(s) => s.to_owned(),
			HtmlItem::Id((id,_)) => format!("{}:{}",id.tb,id.id),
		}
	}
}

