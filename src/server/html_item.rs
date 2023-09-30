use html::inline_text::Anchor;
use surrealdb::sql::Thing;

pub(crate) enum HtmlItem {
	Bool(bool),
	Number(serde_json::Number),
	String(String),
	Id((Thing,Anchor))
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

