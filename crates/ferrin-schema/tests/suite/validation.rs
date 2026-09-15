use ferrin_schema::ValidationIssue;
use ferrin_schema::ValidationIssues;
use pretty_assertions::assert_eq;

#[test]
fn issues_display() {
    assert_eq!(ValidationIssues::default().to_string(), "validation failed");
    assert_eq!(ValidationIssues::message("bad").to_string(), "bad");
    let many = ValidationIssues::new(vec![
        ValidationIssue {
            path: "/a".to_owned(),
            message: "not a string".to_owned(),
        },
        ValidationIssue {
            path: String::new(),
            message: "missing b".to_owned(),
        },
    ]);
    assert_eq!(
        many.to_string(),
        "2 validation issues: [/a: not a string] [missing b]"
    );
}

#[cfg(feature = "json-schema-validation")]
mod dynamic {
    use ferrin_schema::Schema;
    use ferrin_schema::SchemaError;
    use ferrin_schema::validation::Validator;
    use pretty_assertions::assert_eq;
    use serde_json::Value;
    use serde_json::json;

    #[test]
    fn validator_reports_paths() {
        let validator = Validator::compile(&json!({
            "type": "object",
            "properties": { "age": { "type": "integer", "minimum": 0 } },
            "required": ["age"],
            "additionalProperties": false,
        }))
        .unwrap();
        assert!(validator.is_valid(&json!({ "age": 3 })));
        let issues = validator
            .validate(&json!({ "age": -1, "x": 1 }))
            .unwrap_err();
        assert!(
            issues.issues.iter().any(|issue| issue.path == "/age"),
            "{issues}"
        );
        assert!(issues.to_string().contains("validation issues"));
    }

    #[test]
    fn declared_dialect_controls_validation() {
        let modern = json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "array",
            "prefixItems": [{"type": "integer"}],
            "items": false
        });
        let validator = Validator::compile(&modern).unwrap();
        assert!(validator.is_valid(&json!([1])));
        assert!(!validator.is_valid(&json!(["bad"])));
        assert!(!validator.is_valid(&json!([1, 2])));

        let modern = Schema::<Value>::from_json_schema(json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "properties": {"allowed": {"type": "integer"}},
            "unevaluatedProperties": false
        }));
        assert!(modern.validate(json!({"allowed": 1})).is_ok());
        assert!(modern.validate(json!({"extra": 1})).is_err());

        let draft4 = Validator::compile(&json!({
            "$schema": "http://json-schema.org/draft-04/schema#",
            "type": "number", "minimum": 1, "exclusiveMinimum": true
        }))
        .unwrap();
        assert!(draft4.is_valid(&json!(2)));
        assert!(!draft4.is_valid(&json!(1)));
    }

    #[test]
    fn undeclared_schema_uses_draft_seven() {
        // Draft-07 evaluates tuple items and ignores 2020-12 prefixItems.
        let validator = Validator::compile(&json!({
            "type": "array",
            "items": [{"type": "integer"}],
            "additionalItems": false,
            "prefixItems": [{"type": "string"}]
        }))
        .unwrap();
        assert!(validator.is_valid(&json!([1])));
        assert!(!validator.is_valid(&json!(["bad"])));
        assert!(!validator.is_valid(&json!([1, 2])));
    }

    #[test]
    fn invalid_schema_is_reported() {
        let error = Validator::compile(&json!({ "type": 12 })).unwrap_err();
        assert!(matches!(error, SchemaError::InvalidSchema { .. }));
    }

    #[test]
    fn dynamic_schema_validates_values() {
        let schema = Schema::<Value>::from_json_schema(json!({
            "type": "object",
            "properties": { "name": { "type": "string", "minLength": 2 } },
            "required": ["name"],
        }));
        assert_eq!(
            schema.validate(json!({ "name": "ab" })).unwrap(),
            json!({ "name": "ab" })
        );
        let error = schema.validate(json!({ "name": "a" })).unwrap_err();
        assert_eq!(error.value, json!({ "name": "a" }));
        assert!(error.to_string().contains("/name"), "{error}");

        let broken = Schema::<Value>::from_json_schema(json!({ "type": 12 }));
        let error = broken.validate(json!(1)).unwrap_err();
        assert!(error.to_string().contains("invalid json schema"), "{error}");
    }
}
