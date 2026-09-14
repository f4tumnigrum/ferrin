use ferrin_tool::DuplicateToolError;
use ferrin_tool::Schema;
use ferrin_tool::Tool;
use ferrin_tool::ToolSet;
use pretty_assertions::assert_eq;

fn tool() -> Tool {
    Tool::function_with_schema(Schema::empty_object()).build()
}

fn names(set: &ToolSet) -> Vec<&str> {
    set.names().map(ferrin_spec::ToolName::as_str).collect()
}

#[test]
fn insertion_order_and_duplicates() {
    let set = ToolSet::new()
        .insert("zeta", tool())
        .unwrap()
        .insert("alpha", tool())
        .unwrap();
    assert_eq!(names(&set), vec!["zeta", "alpha"]);
    assert_eq!(set.len(), 2);
    assert!(set.contains("alpha"));
    assert!(set.get("beta").is_none());

    let error = set.clone().insert("alpha", tool()).unwrap_err();
    assert_eq!(
        error,
        DuplicateToolError {
            name: "alpha".into()
        }
    );
    let mut mutable = set;
    assert!(mutable.try_insert("gamma", tool()).is_ok());
    assert_eq!(mutable.len(), 3);
    assert!(mutable.remove("zeta").is_some());
    assert_eq!(names(&mutable), vec!["alpha", "gamma"]);
}

#[test]
fn filter_merge_and_order() {
    let set = ToolSet::new()
        .insert("c", tool())
        .unwrap()
        .insert("a", tool())
        .unwrap()
        .insert("b", tool())
        .unwrap();
    let active = set.filter_active(&["b".into(), "c".into(), "missing".into()]);
    assert_eq!(names(&active), vec!["c", "b"]);

    let ordered: Vec<&str> = set
        .ordered(&["b".into()])
        .into_iter()
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(ordered, vec!["b", "a", "c"]);

    let other = ToolSet::new().insert("d", tool()).unwrap();
    let merged = set.clone().merge(other).unwrap();
    assert_eq!(names(&merged), vec!["c", "a", "b", "d"]);
    let clash = ToolSet::new().insert("a", tool()).unwrap();
    assert!(set.merge(clash).is_err());
}
