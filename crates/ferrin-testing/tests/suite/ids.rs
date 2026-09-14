use ferrin_provider_util::IdGenerator;
use ferrin_testing::SequentialIdGenerator;
use pretty_assertions::assert_eq;

#[test]
fn ids_are_sequential_with_prefix() {
    let generator = SequentialIdGenerator::new("call");
    assert_eq!(generator.generate(), "call-0");
    assert_eq!(generator.generate(), "call-1");
    assert_eq!(generator.count(), 2);
    assert_eq!(SequentialIdGenerator::default().generate(), "id-0");
}
