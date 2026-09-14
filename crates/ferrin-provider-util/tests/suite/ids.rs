use std::collections::HashSet;

use ferrin_provider_util::IdGenerator;
use ferrin_provider_util::PrefixedIdGenerator;
use ferrin_provider_util::ids::DEFAULT_ID_ALPHABET;
use ferrin_provider_util::ids::generate_id;
use pretty_assertions::assert_eq;

#[test]
fn prefixed_ids_have_shape_and_alphabet() {
    let generator = PrefixedIdGenerator::new("call", 24);
    let id = generator.generate();
    let random = id.strip_prefix("call-").unwrap();
    assert_eq!(random.len(), 24);
    assert!(random.chars().all(|ch| DEFAULT_ID_ALPHABET.contains(ch)));

    let custom = PrefixedIdGenerator::new("id", 4).with_separator('_');
    assert!(custom.generate().starts_with("id_"));
    assert_eq!(generate_id().len(), 16);
}

#[test]
fn ids_are_unique_enough() {
    let generator = PrefixedIdGenerator::default();
    let ids: HashSet<String> = (0..1000).map(|_| generator.generate()).collect();
    assert_eq!(ids.len(), 1000);
}

#[test]
fn closures_are_generators() {
    let fixed = || "fixed".to_owned();
    let boxed: Box<dyn IdGenerator> = Box::new(fixed);
    assert_eq!(boxed.generate(), "fixed");
}
