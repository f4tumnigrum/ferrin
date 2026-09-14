use pretty_assertions::assert_eq;
use serde_json::json;

use ferrin_spec::language_model::InputTokens;
use ferrin_spec::language_model::OutputTokens;
use ferrin_spec::language_model::Usage;

#[test]
fn add_treats_none_as_absent() {
    let a = Usage {
        input: InputTokens {
            total: Some(10),
            no_cache: None,
            cache_read: Some(2),
            cache_write: None,
        },
        output: OutputTokens {
            total: Some(5),
            text: Some(4),
            reasoning: None,
        },
        raw: Some(json!({ "x": 1 }).as_object().unwrap().clone()),
    };
    let b = Usage {
        input: InputTokens {
            total: Some(1),
            no_cache: Some(1),
            cache_read: None,
            cache_write: None,
        },
        output: OutputTokens {
            total: None,
            text: None,
            reasoning: Some(3),
        },
        raw: None,
    };

    assert_eq!(
        a.add(&b),
        Usage {
            input: InputTokens {
                total: Some(11),
                no_cache: Some(1),
                cache_read: Some(2),
                cache_write: None,
            },
            output: OutputTokens {
                total: Some(5),
                text: Some(4),
                reasoning: Some(3),
            },
            raw: None,
        }
    );
    assert_eq!(a.total_tokens(), Some(15));
    assert_eq!(Usage::default().total_tokens(), None);
}

#[test]
fn serializes_without_absent_counters() {
    let usage = Usage::totals(3, 4);
    assert_eq!(
        serde_json::to_value(&usage).unwrap(),
        json!({ "input": { "total": 3 }, "output": { "total": 4 } })
    );
    let parsed: Usage = serde_json::from_value(json!({})).unwrap();
    assert_eq!(parsed, Usage::default());
}
