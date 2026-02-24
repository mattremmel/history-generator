use history_gen::testutil;

#[test]
fn determinism_same_seed() {
    let world1 = testutil::generate_and_run(99, 50, testutil::core_systems());
    let world2 = testutil::generate_and_run(99, 50, testutil::core_systems());

    testutil::assert_deterministic(&world1, &world2);
}
