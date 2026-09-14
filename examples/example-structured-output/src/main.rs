//! Structured output: the model fills a Rust type described by its JSON
//! Schema, and the result carries the deserialized value.
//!
//! ```text
//! OPENAI_API_KEY=... cargo run -p example-structured-output
//! ```

#![allow(clippy::print_stdout)]

use ferrin::openai::OpenAiSettings;
use ferrin::openai::create_openai;
use ferrin::prelude::*;
use ferrin::provider_util::settings::env_var;

/// A recipe the model has to produce.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(crate = "ferrin::serde")]
#[schemars(crate = "ferrin::schemars")]
struct Recipe {
    /// Dish name.
    name: String,
    /// Ingredients with quantities.
    ingredients: Vec<Ingredient>,
    /// Ordered preparation steps.
    steps: Vec<String>,
    /// Total preparation time in minutes.
    minutes: u32,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(crate = "ferrin::serde")]
#[schemars(crate = "ferrin::schemars")]
struct Ingredient {
    name: String,
    /// Quantity including the unit, e.g. `200 g`.
    quantity: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let openai = create_openai(OpenAiSettings::default())?;
    let model_id = env_var("OPENAI_MODEL").unwrap_or_else(|| "gpt-5".to_owned());

    let result = generate_text(openai.responses(&model_id))
        .prompt("Give me a vegetarian lasagna recipe for four people.")
        .output(Output::<Recipe>::object())
        .await?;

    let recipe = &result.output;
    println!("{} ({} minutes)", recipe.name, recipe.minutes);
    println!();
    println!("Ingredients:");
    for ingredient in &recipe.ingredients {
        println!("- {} {}", ingredient.quantity, ingredient.name);
    }
    println!();
    println!("Steps:");
    for (index, step) in recipe.steps.iter().enumerate() {
        println!("{}. {step}", index + 1);
    }
    Ok(())
}
