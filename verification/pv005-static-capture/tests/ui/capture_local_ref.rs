use pv005_static_capture::Tool;

fn main() {
    let prefix = String::from("pfx");
    let prefix_ref: &str = &prefix;
    let _tool = Tool::function(move |input: String| async move {
        format!("{prefix_ref}:{input}")
    });
}
