use pv005_static_capture::assert_tool_fn;

async fn lookup(db: &str, input: String) -> String {
    format!("{db}:{input}")
}

fn main() {
    let db = String::from("db");
    assert_tool_fn(move |input| lookup(&db, input));
}
