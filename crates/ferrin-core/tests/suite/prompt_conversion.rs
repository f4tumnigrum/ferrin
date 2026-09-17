use std::sync::Arc;

use ferrin_core::generate_text;
use ferrin_core::stream_text;
use ferrin_message::Message;
use ferrin_spec::Prompt;
use ferrin_spec::Usage;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;

use super::common::mock;
use super::common::text_result;

async fn converted(messages: Value, streaming: bool) -> Result<Prompt, ferrin_core::Error> {
    let model = mock()
        .generate(text_result("done"))
        .stream(ferrin_testing::text_parts(["done"], Usage::default()))
        .build_shared();
    let messages: Vec<Message> = serde_json::from_value(messages).unwrap();
    if streaming {
        stream_text(Arc::clone(&model))
            .messages(messages)
            .allow_system_in_messages()
            .await?
            .consume()
            .await?;
        Ok(model.stream_calls()[0].prompt.clone())
    } else {
        generate_text(Arc::clone(&model))
            .messages(messages)
            .allow_system_in_messages()
            .await?;
        Ok(model.generate_calls()[0].prompt.clone())
    }
}

#[tokio::test]
async fn empty_parts_preserve_assistant_options_and_message_boundaries() {
    let messages = json!([
        {"role":"user","content":[{"type":"text","text":"","provider_options":{"test":{"a":1}}},{"type":"text","text":"hi"}]},
        {"role":"assistant","content":[{"type":"text","text":""}]},
        {"role":"assistant","content":[{"type":"text","text":"","provider_options":{"test":{"cache":true}}}]},
        {"role":"user","content":""},
        {"role":"assistant","content":""}
    ]);
    let expected: Prompt = serde_json::from_value(json!([
        {"role":"user","content":[{"type":"text","text":"hi"}]},
        {"role":"assistant","content":[]},
        {"role":"assistant","content":[{"type":"text","text":"","provider_options":{"test":{"cache":true}}}]},
        {"role":"user","content":[{"type":"text","text":""}]},
        {"role":"assistant","content":[{"type":"text","text":""}]}
    ])).unwrap();
    for streaming in [false, true] {
        assert_eq!(
            converted(messages.clone(), streaming).await.unwrap(),
            expected
        );
    }
}

#[tokio::test]
async fn consecutive_tool_messages_move_options_to_parts_with_deep_precedence() {
    let result = |id| json!({"type":"tool-result","tool_call_id":id,"tool_name":"test","output":{"type":"text","value":"done"}});
    let mut first = result("one");
    first["provider_options"] = json!({"test":{"nested":{"replace":"part"},"list":[2]}});
    let messages = json!([
        {"role":"tool","content":[first],"provider_options":{"test":{"nested":{"keep":1,"replace":"message"},"list":[1]}}},
        {"role":"tool","content":[],"provider_options":{"test":{"empty":true}}},
        {"role":"tool","content":[result("two")],"provider_options":{"test":{"last":true}}}
    ]);
    let mut expected_first = result("one");
    expected_first["provider_options"] =
        json!({"test":{"empty":true,"nested":{"keep":1,"replace":"part"},"list":[2]}});
    let expected: Prompt = serde_json::from_value(json!([
        {"role":"tool","content":[expected_first,result("two")],"provider_options":{"test":{"last":true}}}
    ])).unwrap();
    for streaming in [false, true] {
        assert_eq!(
            converted(messages.clone(), streaming).await.unwrap(),
            expected
        );
    }
}

#[tokio::test]
async fn missing_client_tool_results_fail_at_boundaries_and_prompt_end() {
    let call = json!({"type":"tool-call","tool_call_id":"one","tool_name":"test","input":{}});
    for streaming in [false, true] {
        for boundary in [
            None,
            Some(json!({"role":"user","content":"next"})),
            Some(json!({"role":"system","content":"next"})),
        ] {
            let mut messages = vec![json!({"role":"assistant","content":[call]})];
            if let Some(boundary) = boundary {
                messages.push(boundary);
                messages.push(json!({"role":"tool","content":[{"type":"tool-result","tool_call_id":"one","tool_name":"test","output":{"type":"text","value":"too late"}}]}));
            }
            let error = converted(Value::Array(messages), streaming)
                .await
                .unwrap_err();
            assert!(
                error.to_string().contains("missing tool results"),
                "{error}"
            );
        }
        let mut provider_call = call.clone();
        provider_call["provider_executed"] = json!(true);
        let messages = json!([
            {"role":"assistant","content":[provider_call]},
            {"role":"user","content":"next"}
        ]);
        assert_eq!(
            converted(messages.clone(), streaming).await.unwrap(),
            serde_json::from_value::<Prompt>(json!([
                messages[0],{"role":"user","content":[{"type":"text","text":"next"}]}
            ]))
            .unwrap()
        );
    }
}

#[tokio::test]
async fn assistant_file_data_urls_keep_embedded_media_type() {
    for kind in ["file", "reasoning-file"] {
        let messages = json!([{"role":"assistant","content":[{
            "type":kind,"data":{"type":"url","url":"data:text/plain;base64,aGk="},"media_type":"application/json"
        }]}]);
        let expected: Prompt = serde_json::from_value(json!([{"role":"assistant","content":[{
            "type":kind,"data":{"type":"data","data":"aGk="},"media_type":"text/plain"
        }]}]))
        .unwrap();
        for streaming in [false, true] {
            assert_eq!(
                converted(messages.clone(), streaming).await.unwrap(),
                expected
            );
        }
    }
}

#[tokio::test]
async fn reasoning_files_reject_text_and_provider_references() {
    for data in [
        json!({"type":"text","text":"not binary"}),
        json!({"type":"reference","reference":{"test":"file"}}),
    ] {
        for streaming in [false, true] {
            let messages = json!([{"role":"assistant","content":[{
                "type":"reasoning-file","data":data,"media_type":"application/octet-stream"
            }]}]);
            assert!(
                converted(messages, streaming)
                    .await
                    .unwrap_err()
                    .to_string()
                    .contains("reasoning files require")
            );
        }
    }
}
